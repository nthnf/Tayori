use anyhow::Result;
use embedding::Embedder;
use llm::{ChatStream, LlmClient};
use migration::{Migrator, MigratorTrait};
use sea_orm::{Database, EntityTrait};
use std::sync::Arc;
use storage::{
    entities::{documents, projects, sessions, settings, transcript_chunks},
    path::{StorageKind, default_storage_path, ensure_path_exists},
    storage::Storage,
};
use stt::chunk::TranscriptionChunk;

use crate::{
    project,
    retrieval::{self, RetrievalHit},
    session, settings as settings_service, transcript,
    upload::{self, UploadDocumentOptions, UploadedDocument},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectView {
    pub id: String,
    pub name: String,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentView {
    pub id: String,
    pub name: String,
    pub meta: String,
    pub kind: String,
    pub status: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionView {
    pub id: String,
    pub title: String,
    pub meta: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TranscriptChunkView {
    pub id: String,
    pub text: String,
    pub time: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettingsView {
    pub llm_provider: String,
    pub llm_model: String,
    pub embedding_provider: String,
    pub embedding_model: String,
    pub sparse_model: String,
    pub reranker_model: String,
    pub whisper_model: String,
    pub summary_minutes: i64,
    pub ui_theme: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettingsUpdate {
    pub llm_model: String,
    pub embedding_model: String,
    pub sparse_model: String,
    pub reranker_model: String,
    pub whisper_model: String,
    pub summary_minutes: i64,
    pub ui_theme: String,
}

/// Shared application service facade used by UI and workers.
#[derive(Clone)]
pub struct TayoriCore {
    storage: Arc<Storage>,
}

impl TayoriCore {
    /// Run SQLite migrations, open storage, and ensure LanceDB tables/indexes.
    pub async fn bootstrap() -> Result<Self> {
        let sqlite_path = default_storage_path(StorageKind::Sqlite)?;
        ensure_path_exists(StorageKind::Sqlite, &sqlite_path)?;

        let sqlite_uri = format!("sqlite://{sqlite_path}?mode=rwc");
        let sqlite = Database::connect(sqlite_uri).await?;
        Migrator::up(&sqlite, None).await?;
        drop(sqlite);

        let storage = Storage::new().await?;
        Ok(Self {
            storage: Arc::new(storage),
        })
    }

    pub async fn settings(&self) -> Result<SettingsView> {
        settings_service::get_settings(&self.storage)
            .await
            .map(settings_view)
    }

    pub async fn update_settings(&self, update: SettingsUpdate) -> Result<SettingsView> {
        let mut model = settings_service::get_settings(&self.storage).await?;
        model.llm_model = update.llm_model;
        model.llm_store_responses = 1;
        model.embedding_model = update.embedding_model;
        model.sparse_model = update.sparse_model;
        model.reranker_model = update.reranker_model;
        model.whisper_model = optional_string(update.whisper_model);
        model.summary_minutes = update.summary_minutes.clamp(1, 10);
        model.ui_theme = update.ui_theme;

        settings_service::update_settings(&self.storage, model)
            .await
            .map(settings_view)
    }

    pub async fn create_project(
        &self,
        name: impl Into<String>,
        description: Option<String>,
    ) -> Result<ProjectView> {
        project::create_project(&self.storage, name, description)
            .await
            .map(project_view)
    }

    pub async fn list_projects(&self) -> Result<Vec<ProjectView>> {
        Ok(project::list_projects(&self.storage)
            .await?
            .into_iter()
            .map(project_view)
            .collect())
    }

    pub async fn create_session(
        &self,
        project_id: impl Into<String>,
        title: Option<String>,
    ) -> Result<SessionView> {
        session::create_session(&self.storage, project_id, title)
            .await
            .map(session_view)
    }

    pub async fn list_sessions(&self, project_id: &str) -> Result<Vec<SessionView>> {
        Ok(session::list_sessions(&self.storage, project_id)
            .await?
            .into_iter()
            .map(session_view)
            .collect())
    }

    pub async fn end_session(&self, session_id: &str) -> Result<()> {
        session::end_session(&self.storage, session_id).await
    }

    pub async fn list_documents(&self, project_id: &str) -> Result<Vec<DocumentView>> {
        Ok(upload::list_documents(&self.storage, project_id)
            .await?
            .into_iter()
            .map(document_view)
            .collect())
    }

    pub async fn upload_document(
        &self,
        embedder: &mut Embedder,
        project_id: String,
        path: impl AsRef<std::path::Path>,
    ) -> Result<UploadedDocument> {
        upload::upload_document(&self.storage, embedder, project_id, path).await
    }

    pub async fn upload_document_from_settings(
        &self,
        project_id: String,
        path: impl AsRef<std::path::Path>,
    ) -> Result<DocumentView> {
        let settings = settings_service::get_settings(&self.storage).await?;
        let mut embedder = embedder_from_settings(&settings)?;
        let uploaded =
            upload::upload_document(&self.storage, &mut embedder, project_id, path).await?;
        let document = documents::Entity::find_by_id(uploaded.document_id)
            .one(&self.storage.sqlite)
            .await?
            .ok_or_else(|| anyhow::anyhow!("uploaded document row not found"))?;
        Ok(document_view(document))
    }

    pub async fn upload_document_with_options(
        &self,
        embedder: &mut Embedder,
        project_id: String,
        path: impl AsRef<std::path::Path>,
        options: UploadDocumentOptions,
    ) -> Result<UploadedDocument> {
        upload::upload_document_with_options(&self.storage, embedder, project_id, path, options)
            .await
    }

    pub async fn persist_transcription_chunk(
        &self,
        project_id: &str,
        session_id: &str,
        chunk: TranscriptionChunk,
    ) -> Result<TranscriptChunkView> {
        transcript::persist_transcription_chunk(&self.storage, project_id, session_id, chunk)
            .await
            .map(|stored| transcript_chunk_view(stored.chunk))
    }

    pub async fn list_transcript_chunks(
        &self,
        session_id: &str,
    ) -> Result<Vec<TranscriptChunkView>> {
        Ok(
            transcript::list_transcript_chunks(&self.storage, session_id)
                .await?
                .into_iter()
                .map(transcript_chunk_view)
                .collect(),
        )
    }

    pub async fn retrieve_project_context(
        &self,
        embedder: &mut Embedder,
        project_id: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<RetrievalHit>> {
        retrieval::retrieve_project_context(&self.storage, embedder, project_id, query, limit).await
    }

    pub async fn stream_answer(
        &self,
        llm: &LlmClient,
        model: &str,
        question: &str,
        context: &str,
    ) -> Result<ChatStream> {
        crate::answer::stream_answer(llm, model, question, context).await
    }

    pub fn context_text(&self, hits: &[RetrievalHit]) -> String {
        retrieval::format_context(hits)
    }
}

fn project_view(project: projects::Model) -> ProjectView {
    ProjectView {
        id: project.id,
        name: project.name,
        description: project.description.unwrap_or_default(),
    }
}

fn session_view(session: sessions::Model) -> SessionView {
    SessionView {
        id: session.id,
        title: session.title.unwrap_or_else(|| "Live session".to_string()),
        meta: format!(
            "{} • {}",
            session.started_at.format("%b %-d %H:%M"),
            session.status
        ),
    }
}

fn document_view(document: documents::Model) -> DocumentView {
    let kind = document
        .source_name
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_uppercase())
        .unwrap_or_else(|| "Text".to_string());

    DocumentView {
        id: document.id,
        name: document.source_name,
        meta: document
            .original_path
            .unwrap_or_else(|| "Path reference".to_string()),
        kind,
        status: document.status,
    }
}

fn settings_view(settings: settings::Model) -> SettingsView {
    SettingsView {
        llm_provider: settings.llm_provider,
        llm_model: settings.llm_model,
        embedding_provider: settings.embedding_provider,
        embedding_model: settings.embedding_model,
        sparse_model: settings.sparse_model,
        reranker_model: settings.reranker_model,
        whisper_model: settings
            .whisper_model
            .unwrap_or_else(|| stt::path::DEFAULT_WHISPER_MODEL_NAME.to_string()),
        summary_minutes: settings.summary_minutes,
        ui_theme: settings.ui_theme,
    }
}

fn transcript_chunk_view(chunk: transcript_chunks::Model) -> TranscriptChunkView {
    TranscriptChunkView {
        id: chunk.id,
        text: chunk.text,
        time: format_duration(chunk.start_ms),
    }
}

fn format_duration(ms: i64) -> String {
    let total_seconds = ms.max(0) / 1000;
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    format!("{minutes:02}:{seconds:02}")
}

fn optional_string(value: String) -> Option<String> {
    let value = value.trim().to_string();
    if value.is_empty() { None } else { Some(value) }
}

pub fn embedder_from_settings(settings: &settings::Model) -> Result<Embedder> {
    Embedder::new(
        &settings.embedding_model,
        &settings.sparse_model,
        &settings.reranker_model,
    )
}

pub fn llm_from_settings(settings: &settings::Model, api_key: impl Into<String>) -> LlmClient {
    match settings.llm_provider.as_str() {
        "openai" => LlmClient::new(api_key),
        _ => LlmClient::new(api_key),
    }
}
