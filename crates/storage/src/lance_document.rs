use anyhow::{Context, Result, ensure};
use arrow_array::{
    Array, FixedSizeListArray, Int64Array, RecordBatch, StringArray, types::Float32Type,
};
use futures::TryStreamExt;
use lancedb::{
    index::scalar::FullTextSearchQuery,
    query::{ExecutableQuery, QueryBase, Select},
};
use std::sync::Arc;

use crate::lance_schema::document_chunk_schema;
use crate::lance_table::DOCUMENT_CHUNKS_TABLE;
use crate::storage::Storage;

/// Search/index copy of one document chunk stored in LanceDB.
///
/// SQLite owns the durable document record. LanceDB stores this derived row so
/// retrieval can use vector similarity and full-text search over chunk text. If
/// the embedding model changes, these rows can be deleted and rebuilt from the
/// SQLite source data.
#[derive(Clone, Debug, PartialEq)]
pub struct LanceDocumentChunk {
    /// Stable chunk row ID in LanceDB.
    pub id: String,
    /// Project scope for filtering searches.
    pub project_id: String,
    /// SQLite document ID this chunk belongs to.
    pub document_id: String,
    /// Monotonic chunk index inside the document.
    pub chunk_index: i64,
    /// Chunk body used for full-text search and context display.
    pub text: String,
    /// Embedding vector for semantic search.
    pub vector: Vec<f32>,
    /// Creation time as unix seconds.
    pub created_at_unix_secs: i64,
}

/// Typed document chunk search result returned by storage callers.
///
/// LanceDB query results arrive as Arrow record batches. Storage converts those
/// batches into this stable Rust type so core/app code does not need to know
/// about Arrow column arrays.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LanceDocumentChunkResult {
    pub id: String,
    pub project_id: String,
    pub document_id: String,
    pub chunk_index: i64,
    pub text: String,
    pub created_at_unix_secs: i64,
}

impl Storage {
    /// Insert one document chunk into LanceDB.
    ///
    /// The vector length is validated against `settings.embedding_dimension`
    /// before writing, because LanceDB requires a fixed vector length per table.
    pub async fn add_lance_document_chunk(&self, row: LanceDocumentChunk) -> Result<()> {
        let embedding_dimension = self.embedding_dimension().await?;
        ensure!(
            row.vector.len() == embedding_dimension as usize,
            "document chunk vector length {} does not match embedding dimension {}",
            row.vector.len(),
            embedding_dimension
        );

        let table = self
            .lancedb
            .open_table(DOCUMENT_CHUNKS_TABLE)
            .execute()
            .await?;
        table
            .add(document_chunk_batch(row, embedding_dimension)?)
            .execute()
            .await?;

        Ok(())
    }

    /// Delete one LanceDB document chunk by its Lance row ID.
    pub async fn delete_lance_document_chunk(&self, id: &str) -> Result<()> {
        let table = self
            .lancedb
            .open_table(DOCUMENT_CHUNKS_TABLE)
            .execute()
            .await?;
        table
            .delete(&format!("id = '{}'", escape_sql_string(id)))
            .await?;

        Ok(())
    }

    /// Delete all LanceDB chunks for one SQLite document.
    ///
    /// This is used when a document is re-ingested or removed. SQLite remains the
    /// source of truth; this only deletes the rebuildable search projection.
    pub async fn delete_lance_document_chunks_for_document(
        &self,
        project_id: &str,
        document_id: &str,
    ) -> Result<()> {
        let table = self
            .lancedb
            .open_table(DOCUMENT_CHUNKS_TABLE)
            .execute()
            .await?;
        table
            .delete(&format!(
                "project_id = '{}' AND document_id = '{}'",
                escape_sql_string(project_id),
                escape_sql_string(document_id)
            ))
            .await?;

        Ok(())
    }

    /// Search document chunks by vector similarity inside a project.
    pub async fn search_lance_document_chunks_by_vector(
        &self,
        project_id: &str,
        vector: &[f32],
        limit: usize,
    ) -> Result<Vec<LanceDocumentChunkResult>> {
        let embedding_dimension = self.embedding_dimension().await?;
        ensure!(
            vector.len() == embedding_dimension as usize,
            "query vector length {} does not match embedding dimension {}",
            vector.len(),
            embedding_dimension
        );

        let table = self
            .lancedb
            .open_table(DOCUMENT_CHUNKS_TABLE)
            .execute()
            .await?;
        let batches = table
            .query()
            .nearest_to(vector)?
            .select(Select::Columns(document_chunk_result_columns()))
            .only_if(format!("project_id = '{}'", escape_sql_string(project_id)))
            .limit(limit)
            .execute()
            .await?
            .try_collect::<Vec<_>>()
            .await?;

        document_chunk_results_from_batches(&batches)
    }

    /// Search document chunks by full-text query inside a project.
    pub async fn search_lance_document_chunks_by_text(
        &self,
        project_id: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<LanceDocumentChunkResult>> {
        let table = self
            .lancedb
            .open_table(DOCUMENT_CHUNKS_TABLE)
            .execute()
            .await?;
        let batches = table
            .query()
            .full_text_search(FullTextSearchQuery::new(query.to_owned()))
            .only_if(format!("project_id = '{}'", escape_sql_string(project_id)))
            .select(Select::Columns(document_chunk_result_columns()))
            .limit(limit)
            .execute()
            .await?
            .try_collect::<Vec<_>>()
            .await?;

        document_chunk_results_from_batches(&batches)
    }
}

fn document_chunk_result_columns() -> Vec<String> {
    [
        "id",
        "project_id",
        "document_id",
        "chunk_index",
        "text",
        "created_at",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn document_chunk_results_from_batches(
    batches: &[RecordBatch],
) -> Result<Vec<LanceDocumentChunkResult>> {
    let mut results = Vec::new();

    for batch in batches {
        let ids = string_column(batch, "id")?;
        let project_ids = string_column(batch, "project_id")?;
        let document_ids = string_column(batch, "document_id")?;
        let chunk_indexes = int64_column(batch, "chunk_index")?;
        let texts = string_column(batch, "text")?;
        let created_ats = int64_column(batch, "created_at")?;

        for row in 0..batch.num_rows() {
            results.push(LanceDocumentChunkResult {
                id: ids.value(row).to_owned(),
                project_id: project_ids.value(row).to_owned(),
                document_id: document_ids.value(row).to_owned(),
                chunk_index: chunk_indexes.value(row),
                text: texts.value(row).to_owned(),
                created_at_unix_secs: created_ats.value(row),
            });
        }
    }

    Ok(results)
}

fn string_column<'a>(batch: &'a RecordBatch, name: &str) -> Result<&'a StringArray> {
    batch
        .column_by_name(name)
        .with_context(|| format!("missing LanceDB string column: {name}"))?
        .as_any()
        .downcast_ref::<StringArray>()
        .with_context(|| format!("LanceDB column is not Utf8: {name}"))
}

fn int64_column<'a>(batch: &'a RecordBatch, name: &str) -> Result<&'a Int64Array> {
    batch
        .column_by_name(name)
        .with_context(|| format!("missing LanceDB int64 column: {name}"))?
        .as_any()
        .downcast_ref::<Int64Array>()
        .with_context(|| format!("LanceDB column is not Int64: {name}"))
}

fn document_chunk_batch(row: LanceDocumentChunk, embedding_dimension: i32) -> Result<RecordBatch> {
    let schema = document_chunk_schema(embedding_dimension);
    let vector = row.vector.into_iter().map(Some).collect::<Vec<_>>();

    Ok(RecordBatch::try_new(
        schema,
        vec![
            Arc::new(StringArray::from(vec![row.id])),
            Arc::new(StringArray::from(vec![row.project_id])),
            Arc::new(StringArray::from(vec![row.document_id])),
            Arc::new(Int64Array::from(vec![row.chunk_index])),
            Arc::new(StringArray::from(vec![row.text])),
            Arc::new(
                FixedSizeListArray::from_iter_primitive::<Float32Type, _, _>(
                    [Some(vector)],
                    embedding_dimension,
                ),
            ),
            Arc::new(Int64Array::from(vec![row.created_at_unix_secs])),
        ],
    )?)
}

fn escape_sql_string(value: &str) -> String {
    value.replace('\'', "''")
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::TryStreamExt;
    use lancedb::connect;
    use lancedb::index::Index;

    const DIM: i32 = 4;

    #[tokio::test]
    async fn document_chunk_can_be_added_searched_and_deleted() {
        let db = connect("memory://").execute().await.unwrap();
        let table = db
            .create_empty_table(DOCUMENT_CHUNKS_TABLE, document_chunk_schema(DIM))
            .execute()
            .await
            .unwrap();

        table
            .add(
                document_chunk_batch(
                    LanceDocumentChunk {
                        id: "chunk-1".to_string(),
                        project_id: "project-1".to_string(),
                        document_id: "doc-1".to_string(),
                        chunk_index: 0,
                        text: "rust ownership and borrowing".to_string(),
                        vector: vec![1.0, 0.0, 0.0, 0.0],
                        created_at_unix_secs: 1_700_000_000,
                    },
                    DIM,
                )
                .unwrap(),
            )
            .execute()
            .await
            .unwrap();

        table
            .create_index(&["text"], Index::FTS(Default::default()))
            .execute()
            .await
            .unwrap();
        let vector_batches = table
            .query()
            .nearest_to(&[1.0, 0.0, 0.0, 0.0])
            .unwrap()
            .only_if("project_id = 'project-1'")
            .limit(1)
            .execute()
            .await
            .unwrap()
            .try_collect::<Vec<_>>()
            .await
            .unwrap();
        assert_eq!(
            vector_batches
                .iter()
                .map(|batch| batch.num_rows())
                .sum::<usize>(),
            1
        );
        let vector_results = document_chunk_results_from_batches(&vector_batches).unwrap();
        assert_eq!(vector_results[0].id, "chunk-1");
        assert_eq!(vector_results[0].text, "rust ownership and borrowing");

        let text_batches = table
            .query()
            .full_text_search(FullTextSearchQuery::new("ownership".to_string()))
            .only_if("project_id = 'project-1'")
            .limit(1)
            .execute()
            .await
            .unwrap()
            .try_collect::<Vec<_>>()
            .await
            .unwrap();
        assert_eq!(
            text_batches
                .iter()
                .map(|batch| batch.num_rows())
                .sum::<usize>(),
            1
        );
        let text_results = document_chunk_results_from_batches(&text_batches).unwrap();
        assert_eq!(text_results[0].document_id, "doc-1");

        table.delete("id = 'chunk-1'").await.unwrap();

        let batches = table
            .query()
            .limit(10)
            .execute()
            .await
            .unwrap()
            .try_collect::<Vec<_>>()
            .await
            .unwrap();
        assert_eq!(
            batches.iter().map(|batch| batch.num_rows()).sum::<usize>(),
            0
        );
    }
}
