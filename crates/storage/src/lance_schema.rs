use lancedb::arrow::arrow_schema::{DataType, Field, Schema};
use std::sync::Arc;

/// Build the LanceDB schema for transcript summary search rows.
///
/// `embedding_dimension` must match the active embedding model. LanceDB stores
/// vectors as `FixedSizeList<Float32, embedding_dimension>`, so every inserted
/// vector must have exactly this length. The table intentionally duplicates the
/// summary text from SQLite so LanceDB can provide full-text search without a
/// SQLite round trip.
pub fn transcript_summary_schema(embedding_dimension: i32) -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("id", DataType::Utf8, false),
        Field::new("project_id", DataType::Utf8, false),
        Field::new("session_id", DataType::Utf8, false),
        Field::new("summary_index", DataType::Int64, false),
        Field::new("chunk_start_index", DataType::Int64, false),
        Field::new("chunk_end_index", DataType::Int64, false),
        Field::new("start_ms", DataType::Int64, false),
        Field::new("end_ms", DataType::Int64, false),
        Field::new("duration_ms", DataType::Int64, false),
        Field::new("summary", DataType::Utf8, false),
        Field::new(
            "vector",
            DataType::FixedSizeList(
                Arc::new(Field::new("item", DataType::Float32, true)),
                embedding_dimension,
            ),
            false,
        ),
        Field::new("created_at", DataType::Int64, false),
    ]))
}

/// Build the LanceDB schema for document chunk search rows.
///
/// Documents are chunked before embedding. Each row stores one chunk's text and
/// vector plus IDs needed to map the result back to SQLite document metadata.
/// The text is duplicated in LanceDB so document chunks can use full-text search
/// and vector search from the same table.
pub fn document_chunk_schema(embedding_dimension: i32) -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("id", DataType::Utf8, false),
        Field::new("project_id", DataType::Utf8, false),
        Field::new("document_id", DataType::Utf8, false),
        Field::new("chunk_index", DataType::Int64, false),
        Field::new("text", DataType::Utf8, false),
        Field::new(
            "vector",
            DataType::FixedSizeList(
                Arc::new(Field::new("item", DataType::Float32, true)),
                embedding_dimension,
            ),
            false,
        ),
        Field::new("created_at", DataType::Int64, false),
    ]))
}
