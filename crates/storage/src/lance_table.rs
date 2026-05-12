use anyhow::{Result, ensure};
use lancedb::{Table, database::CreateTableMode, index::Index};
use sea_orm::EntityTrait;

use crate::entities::settings;
use crate::lance_schema::{document_chunk_schema, transcript_summary_schema};
use crate::storage::Storage;

/// LanceDB table that stores embedded document chunks.
///
/// The raw document metadata lives in SQLite. This table is a rebuildable search
/// projection containing the chunk text, vector, and enough IDs to map results
/// back to SQLite documents.
pub const DOCUMENT_CHUNKS_TABLE: &str = "document_chunks";

/// LanceDB table that stores embedded transcript summaries.
///
/// Raw transcript chunks stay in SQLite. This table stores summary windows only,
/// which keeps retrieval compact and enables full-text search over the generated
/// summary text.
pub const TRANSCRIPT_SUMMARIES_TABLE: &str = "transcript_summaries";

impl Storage {
    pub(super) async fn ensure_lance_schema(&self) -> Result<()> {
        self.ensure_lance_tables().await?;
        self.ensure_lance_indexes().await?;

        Ok(())
    }

    async fn ensure_lance_tables(&self) -> Result<()> {
        let embedding_dimension = self.embedding_dimension().await?;
        let docs_schema = document_chunk_schema(embedding_dimension);
        let summary_schema = transcript_summary_schema(embedding_dimension);

        self.lancedb
            .create_empty_table(DOCUMENT_CHUNKS_TABLE, docs_schema)
            .mode(CreateTableMode::exist_ok(|request| request))
            .execute()
            .await?;

        self.lancedb
            .create_empty_table(TRANSCRIPT_SUMMARIES_TABLE, summary_schema)
            .mode(CreateTableMode::exist_ok(|request| request))
            .execute()
            .await?;

        Ok(())
    }

    async fn ensure_lance_indexes(&self) -> Result<()> {
        // LanceDB indexes are per table/column. We keep startup idempotent by
        // checking existing indexes before creating missing ones below.
        self.ensure_lance_document_chunk_indexes().await?;
        self.ensure_lance_transcript_summary_indexes().await?;

        Ok(())
    }

    async fn ensure_lance_document_chunk_indexes(&self) -> Result<()> {
        let table = self
            .lancedb
            .open_table(DOCUMENT_CHUNKS_TABLE)
            .execute()
            .await?;

        ensure_vector_index(&table, "vector").await?;
        ensure_index(
            &table,
            "text",
            Index::FTS(lancedb::index::scalar::FtsIndexBuilder::default()),
        )
        .await?;

        Ok(())
    }

    async fn ensure_lance_transcript_summary_indexes(&self) -> Result<()> {
        let table = self
            .lancedb
            .open_table(TRANSCRIPT_SUMMARIES_TABLE)
            .execute()
            .await?;

        ensure_vector_index(&table, "vector").await?;
        ensure_index(
            &table,
            "summary",
            Index::FTS(lancedb::index::scalar::FtsIndexBuilder::default()),
        )
        .await?;

        Ok(())
    }

    pub(super) async fn embedding_dimension(&self) -> Result<i32> {
        let settings = settings::Entity::find_by_id("default")
            .one(&self.sqlite)
            .await?
            .ok_or_else(|| anyhow::anyhow!("default settings row not found"))?;

        ensure!(
            settings.embedding_dimension > 0 && settings.embedding_dimension <= i32::MAX as i64,
            "invalid embedding dimension: {}",
            settings.embedding_dimension
        );

        Ok(settings.embedding_dimension as i32)
    }
}

async fn ensure_index(table: &Table, column: &str, index: Index) -> Result<()> {
    // LanceDB returns the list of already-created indexes for this table. Each
    // index config includes the indexed columns. We only need one index per text
    // column (`text` for documents, `summary` for transcript summaries), so if
    // any existing index already covers this column, there is nothing to do.
    let indexes = table.list_indices().await?;
    if indexes.iter().any(|index| {
        index
            .columns
            .iter()
            .any(|indexed_column| indexed_column == column)
    }) {
        return Ok(());
    }

    // `Index::FTS(...)` builds a full-text inverted index. Without this index,
    // LanceDB full-text search either cannot run efficiently or may not use the
    // expected FTS query path. This is safe to create at startup for empty/small
    // tables because FTS does not need vector/PQ training data.
    table.create_index(&[column], index).execute().await?;

    Ok(())
}

async fn ensure_vector_index(table: &Table, column: &str) -> Result<()> {
    // Vector search can work in two modes:
    //
    // 1. No vector index: LanceDB scans rows directly. This is fine for small
    //    tables and fresh installs.
    // 2. Vector index: LanceDB builds an ANN index for faster nearest-neighbor
    //    search once there is enough data.
    //
    // Like text indexes, vector indexes are per table/column. If `vector` is
    // already indexed, skip creation so startup stays idempotent.
    let indexes = table.list_indices().await?;
    if indexes.iter().any(|index| {
        index
            .columns
            .iter()
            .any(|indexed_column| indexed_column == column)
    }) {
        return Ok(());
    }

    // `Index::Auto` currently chooses a PQ-based ANN index internally. PQ needs
    // training samples and LanceDB errors when fewer than 256 rows exist. Fresh
    // tables start empty, and early projects may have only a few chunks, so we
    // skip vector index creation until the table is large enough.
    //
    // This does not disable vector search. Queries using `.nearest_to(...)` still
    // work before the index exists; they just use brute-force search. Later, a
    // startup/rebuild pass can create the ANN index once row count is sufficient.
    if table.count_rows(None).await? < 256 {
        return Ok(());
    }

    // Create the vector index only after the table has enough rows to train it.
    table.create_index(&[column], Index::Auto).execute().await?;

    Ok(())
}
