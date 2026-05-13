use anyhow::{Context, Result, ensure};
use chrono::Utc;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use sha2::{Digest, Sha256};
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use storage::{entities::documents, lance_document::LanceDocumentChunk, storage::Storage};
use uuid::Uuid;

const DEFAULT_CHUNK_MAX_CHARS: usize = 2_400;
const DEFAULT_CHUNK_OVERLAP_CHARS: usize = 240;

/// Async embedding provider used by document upload.
///
/// Core owns ingestion orchestration, but the actual embedding model/client is
/// injected so this crate does not depend on one concrete embedding backend.
pub trait DocumentEmbedder: Send + Sync {
    fn embed<'a>(
        &'a self,
        text: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<DocumentEmbedding>> + Send + 'a>>;
}

/// Dense and sparse embeddings for one chunk.
#[derive(Clone, Debug, PartialEq)]
pub struct DocumentEmbedding {
    pub dense_vector: Vec<f32>,
    pub sparse_indices: Vec<u32>,
    pub sparse_values: Vec<f32>,
}

/// Result of ingesting one local document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UploadedDocument {
    pub document_id: String,
    pub chunks: usize,
    pub skipped_existing: bool,
}

/// Configuration for local document upload.
#[derive(Clone, Debug)]
pub struct UploadDocumentOptions {
    pub chunk_max_chars: usize,
    pub chunk_overlap_chars: usize,
}

impl Default for UploadDocumentOptions {
    fn default() -> Self {
        Self {
            chunk_max_chars: DEFAULT_CHUNK_MAX_CHARS,
            chunk_overlap_chars: DEFAULT_CHUNK_OVERLAP_CHARS,
        }
    }
}

/// Ingest a local text-like document into SQLite metadata and LanceDB chunks.
///
/// The original file is not copied. SQLite stores its path and content hash;
/// LanceDB stores chunk text plus vectors so search still works even if the
/// source file is later moved or deleted.
pub async fn upload_document(
    storage: &Storage,
    embedder: &dyn DocumentEmbedder,
    project_id: String,
    path: impl AsRef<Path>,
) -> Result<UploadedDocument> {
    upload_document_with_options(
        storage,
        embedder,
        project_id,
        path,
        UploadDocumentOptions::default(),
    )
    .await
}

/// Ingest a local document with explicit chunking options.
pub async fn upload_document_with_options(
    storage: &Storage,
    embedder: &dyn DocumentEmbedder,
    project_id: String,
    path: impl AsRef<Path>,
    options: UploadDocumentOptions,
) -> Result<UploadedDocument> {
    ensure!(options.chunk_max_chars > 0, "chunk_max_chars must be > 0");
    ensure!(
        options.chunk_overlap_chars < options.chunk_max_chars,
        "chunk_overlap_chars must be smaller than chunk_max_chars"
    );

    let path = path.as_ref();
    ensure!(path.is_file(), "document path is not a file: {path:?}");
    ensure_supported_extension(path)?;

    let bytes =
        std::fs::read(path).with_context(|| format!("failed to read document: {path:?}"))?;
    let content_hash = content_hash_hex(&bytes);

    if let Some(existing) =
        find_existing_ready_document(storage, &project_id, &content_hash).await?
    {
        return Ok(UploadedDocument {
            document_id: existing.id,
            chunks: 0,
            skipped_existing: true,
        });
    }

    let document_id = Uuid::new_v4().to_string();
    let source_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("document")
        .to_string();
    let original_path = path.to_string_lossy().to_string();

    insert_ingesting_document(
        storage,
        &document_id,
        &project_id,
        &source_name,
        &original_path,
        &content_hash,
    )
    .await?;

    match ingest_document_chunks(
        storage,
        embedder,
        &document_id,
        &project_id,
        &bytes,
        options,
    )
    .await
    {
        Ok(chunks) => {
            mark_document_ready(storage, &document_id).await?;
            Ok(UploadedDocument {
                document_id,
                chunks,
                skipped_existing: false,
            })
        }
        Err(error) => {
            let _ = storage
                .delete_lance_document_chunks_for_document(&project_id, &document_id)
                .await;
            let _ = mark_document_failed(storage, &document_id, &error.to_string()).await;
            Err(error)
        }
    }
}

async fn ingest_document_chunks(
    storage: &Storage,
    embedder: &dyn DocumentEmbedder,
    document_id: &str,
    project_id: &str,
    bytes: &[u8],
    options: UploadDocumentOptions,
) -> Result<usize> {
    let text = String::from_utf8(bytes.to_vec()).context("document is not valid UTF-8")?;
    let chunks = chunk_text(&text, options.chunk_max_chars, options.chunk_overlap_chars);
    ensure!(!chunks.is_empty(), "document contains no text to ingest");

    for (index, chunk) in chunks.iter().enumerate() {
        let embedding = embedder.embed(chunk).await?;
        storage
            .add_lance_document_chunk(LanceDocumentChunk {
                id: Uuid::new_v4().to_string(),
                project_id: project_id.to_string(),
                document_id: document_id.to_string(),
                chunk_index: index as i64,
                text: chunk.clone(),
                dense_vector: embedding.dense_vector,
                sparse_indices: embedding.sparse_indices,
                sparse_values: embedding.sparse_values,
                created_at_unix_secs: Utc::now().timestamp(),
            })
            .await?;
    }

    Ok(chunks.len())
}

async fn find_existing_ready_document(
    storage: &Storage,
    project_id: &str,
    content_hash: &str,
) -> Result<Option<documents::Model>> {
    documents::Entity::find()
        .filter(documents::Column::ProjectId.eq(project_id))
        .filter(documents::Column::ContentHash.eq(content_hash))
        .filter(documents::Column::Status.eq("ready"))
        .one(&storage.sqlite)
        .await
        .map_err(Into::into)
}

async fn insert_ingesting_document(
    storage: &Storage,
    document_id: &str,
    project_id: &str,
    source_name: &str,
    original_path: &str,
    content_hash: &str,
) -> Result<()> {
    let now = Utc::now();
    documents::ActiveModel {
        id: Set(document_id.to_string()),
        project_id: Set(project_id.to_string()),
        source_name: Set(source_name.to_string()),
        original_path: Set(Some(original_path.to_string())),
        content_hash: Set(Some(content_hash.to_string())),
        status: Set("ingesting".to_string()),
        error_message: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(&storage.sqlite)
    .await?;

    Ok(())
}

async fn mark_document_ready(storage: &Storage, document_id: &str) -> Result<()> {
    let document = documents::Entity::find_by_id(document_id)
        .one(&storage.sqlite)
        .await?
        .ok_or_else(|| anyhow::anyhow!("document row not found: {document_id}"))?;
    let mut document: documents::ActiveModel = document.into();
    document.status = Set("ready".to_string());
    document.error_message = Set(None);
    document.updated_at = Set(Utc::now());
    document.update(&storage.sqlite).await?;
    Ok(())
}

async fn mark_document_failed(storage: &Storage, document_id: &str, error: &str) -> Result<()> {
    let document = documents::Entity::find_by_id(document_id)
        .one(&storage.sqlite)
        .await?
        .ok_or_else(|| anyhow::anyhow!("document row not found: {document_id}"))?;
    let mut document: documents::ActiveModel = document.into();
    document.status = Set("failed".to_string());
    document.error_message = Set(Some(error.to_string()));
    document.updated_at = Set(Utc::now());
    document.update(&storage.sqlite).await?;
    Ok(())
}

fn ensure_supported_extension(path: &Path) -> Result<()> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    ensure!(
        matches!(extension.as_str(), "txt" | "md" | "markdown"),
        "unsupported document extension: {extension}"
    );
    Ok(())
}

fn content_hash_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn chunk_text(text: &str, max_chars: usize, overlap_chars: usize) -> Vec<String> {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return Vec::new();
    }

    let chars = normalized.chars().collect::<Vec<_>>();
    let mut chunks = Vec::new();
    let mut start = 0;

    while start < chars.len() {
        let mut end = (start + max_chars).min(chars.len());
        if end < chars.len() {
            end = find_chunk_boundary(&chars, start, end).max(start + 1);
        }

        let chunk = chars[start..end]
            .iter()
            .collect::<String>()
            .trim()
            .to_string();
        if !chunk.is_empty() {
            chunks.push(chunk);
        }

        if end == chars.len() {
            break;
        }
        start = end.saturating_sub(overlap_chars);
    }

    chunks
}

fn find_chunk_boundary(chars: &[char], start: usize, fallback_end: usize) -> usize {
    chars[start..fallback_end]
        .iter()
        .rposition(|c| matches!(c, '.' | '!' | '?' | '\n'))
        .map(|offset| start + offset + 1)
        .unwrap_or(fallback_end)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn chunk_text_splits_with_overlap() {
        let chunks = chunk_text("one two three four five six seven", 14, 4);

        assert!(chunks.len() > 1);
        assert!(chunks[0].len() <= 14);
    }

    #[test]
    fn content_hash_is_stable() {
        assert_eq!(content_hash_hex(b"abc"), content_hash_hex(b"abc"));
    }

    #[test]
    fn supported_extensions_are_text_like() {
        ensure_supported_extension(&PathBuf::from("notes.md")).unwrap();
        ensure_supported_extension(&PathBuf::from("notes.txt")).unwrap();
        assert!(ensure_supported_extension(&PathBuf::from("notes.pdf")).is_err());
    }
}
