use lancedb::arrow::arrow_schema::{DataType, Field, Schema};
use std::sync::Arc;

/// Build the LanceDB schema for transcript summary search rows.
///
/// `embedding_dimension` must match the active dense embedding model. Dense
/// vectors are fixed-size. Sparse vectors are stored losslessly as matching
/// index/value lists because their active dimensions vary per input.
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
            "dense_vector",
            DataType::FixedSizeList(
                Arc::new(Field::new("item", DataType::Float32, true)),
                embedding_dimension,
            ),
            false,
        ),
        Field::new(
            "sparse_indices",
            DataType::List(Arc::new(Field::new("item", DataType::UInt32, true))),
            false,
        ),
        Field::new(
            "sparse_values",
            DataType::List(Arc::new(Field::new("item", DataType::Float32, true))),
            false,
        ),
        Field::new("created_at", DataType::Int64, false),
    ]))
}

/// Build the LanceDB schema for document chunk search rows.
///
/// Documents are chunked before embedding. Each row stores one chunk's text,
/// dense vector, sparse vector, and IDs needed to map the result back to SQLite
/// document metadata.
pub fn document_chunk_schema(embedding_dimension: i32) -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("id", DataType::Utf8, false),
        Field::new("project_id", DataType::Utf8, false),
        Field::new("document_id", DataType::Utf8, false),
        Field::new("chunk_index", DataType::Int64, false),
        Field::new("text", DataType::Utf8, false),
        Field::new(
            "dense_vector",
            DataType::FixedSizeList(
                Arc::new(Field::new("item", DataType::Float32, true)),
                embedding_dimension,
            ),
            false,
        ),
        Field::new(
            "sparse_indices",
            DataType::List(Arc::new(Field::new("item", DataType::UInt32, true))),
            false,
        ),
        Field::new(
            "sparse_values",
            DataType::List(Arc::new(Field::new("item", DataType::Float32, true))),
            false,
        ),
        Field::new("created_at", DataType::Int64, false),
    ]))
}
