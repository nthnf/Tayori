use anyhow::{Context, Result, ensure};
use arrow_array::{
    Array, FixedSizeListArray, Float32Array, Int64Array, ListArray, RecordBatch, StringArray,
    UInt32Array,
    types::{Float32Type, UInt32Type},
};
use futures::TryStreamExt;
use lancedb::{
    index::scalar::FullTextSearchQuery,
    query::{ExecutableQuery, QueryBase, Select},
};
use std::{collections::HashSet, sync::Arc};

use crate::lance_schema::transcript_summary_schema;
use crate::lance_table::TRANSCRIPT_SUMMARIES_TABLE;
use crate::storage::Storage;

/// Search/index copy of one transcript summary window stored in LanceDB.
///
/// SQLite owns raw transcript chunks and durable summary rows. LanceDB stores a
/// derived copy so retrieval can search compact meeting windows instead of every
/// raw STT chunk. If the embedding model changes, this table can be rebuilt from
/// SQLite summaries.
#[derive(Clone, Debug, PartialEq)]
pub struct LanceTranscriptSummary {
    /// Stable summary row ID in LanceDB.
    pub id: String,
    /// Project scope for filtering searches.
    pub project_id: String,
    /// SQLite session ID this summary belongs to.
    pub session_id: String,
    /// Monotonic summary index inside the session.
    pub summary_index: i64,
    /// First transcript chunk covered by this summary.
    pub chunk_start_index: i64,
    /// Last transcript chunk covered by this summary.
    pub chunk_end_index: i64,
    /// Stream-relative start time of this summary window.
    pub start_ms: i64,
    /// Stream-relative end time of this summary window.
    pub end_ms: i64,
    /// Duration covered by this summary window.
    pub duration_ms: i64,
    /// Summary text used for full-text search and context display.
    pub summary: String,
    /// Dense embedding vector for semantic search.
    pub dense_vector: Vec<f32>,
    /// Sparse embedding dimensions with non-zero weights.
    pub sparse_indices: Vec<u32>,
    /// Sparse embedding weights aligned with `sparse_indices`.
    pub sparse_values: Vec<f32>,
    /// Creation time as unix seconds.
    pub created_at_unix_secs: i64,
}

/// Typed transcript summary search result returned by storage callers.
///
/// LanceDB query results arrive as Arrow record batches. Storage converts those
/// batches into this stable Rust type so core/app code can consume summaries
/// without Arrow-specific parsing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LanceTranscriptSummaryResult {
    pub id: String,
    pub project_id: String,
    pub session_id: String,
    pub summary_index: i64,
    pub chunk_start_index: i64,
    pub chunk_end_index: i64,
    pub start_ms: i64,
    pub end_ms: i64,
    pub duration_ms: i64,
    pub summary: String,
    pub created_at_unix_secs: i64,
}

impl Storage {
    /// Insert one transcript summary into LanceDB.
    ///
    /// The vector length is validated against `settings.embedding_dimension`
    /// before writing, because LanceDB requires a fixed vector length per table.
    pub async fn add_lance_transcript_summary(&self, row: LanceTranscriptSummary) -> Result<()> {
        let embedding_dimension = self.embedding_dimension().await?;
        ensure!(
            row.dense_vector.len() == embedding_dimension as usize,
            "transcript summary dense vector length {} does not match embedding dimension {}",
            row.dense_vector.len(),
            embedding_dimension
        );
        ensure!(
            row.sparse_indices.len() == row.sparse_values.len(),
            "transcript summary sparse indices length {} does not match sparse values length {}",
            row.sparse_indices.len(),
            row.sparse_values.len()
        );

        let table = self
            .lancedb
            .open_table(TRANSCRIPT_SUMMARIES_TABLE)
            .execute()
            .await?;
        table
            .add(transcript_summary_batch(row, embedding_dimension)?)
            .execute()
            .await?;

        Ok(())
    }

    /// Delete one LanceDB transcript summary by its Lance row ID.
    pub async fn delete_lance_transcript_summary(&self, id: &str) -> Result<()> {
        let table = self
            .lancedb
            .open_table(TRANSCRIPT_SUMMARIES_TABLE)
            .execute()
            .await?;
        table
            .delete(&format!("id = '{}'", escape_sql_string(id)))
            .await?;

        Ok(())
    }

    /// Delete all LanceDB transcript summaries for one session.
    ///
    /// Used by summary rebuild flows. SQLite summaries remain the durable source;
    /// this only deletes the rebuildable search projection.
    pub async fn delete_lance_transcript_summaries_for_session(
        &self,
        project_id: &str,
        session_id: &str,
    ) -> Result<()> {
        let table = self
            .lancedb
            .open_table(TRANSCRIPT_SUMMARIES_TABLE)
            .execute()
            .await?;
        table
            .delete(&format!(
                "project_id = '{}' AND session_id = '{}'",
                escape_sql_string(project_id),
                escape_sql_string(session_id)
            ))
            .await?;

        Ok(())
    }

    /// Search transcript summaries by vector similarity inside a project.
    pub async fn search_lance_transcript_summaries_by_vector(
        &self,
        project_id: &str,
        vector: &[f32],
        limit: usize,
    ) -> Result<Vec<LanceTranscriptSummaryResult>> {
        let embedding_dimension = self.embedding_dimension().await?;
        ensure!(
            vector.len() == embedding_dimension as usize,
            "query vector length {} does not match embedding dimension {}",
            vector.len(),
            embedding_dimension
        );

        let table = self
            .lancedb
            .open_table(TRANSCRIPT_SUMMARIES_TABLE)
            .execute()
            .await?;
        let batches = table
            .query()
            .nearest_to(vector)?
            .column("dense_vector")
            .select(Select::Columns(transcript_summary_result_columns()))
            .only_if(format!("project_id = '{}'", escape_sql_string(project_id)))
            .limit(limit)
            .execute()
            .await?
            .try_collect::<Vec<_>>()
            .await?;

        transcript_summary_results_from_batches(&batches)
    }

    /// Search transcript summaries by full-text query inside a project.
    pub async fn search_lance_transcript_summaries_by_text(
        &self,
        project_id: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<LanceTranscriptSummaryResult>> {
        let table = self
            .lancedb
            .open_table(TRANSCRIPT_SUMMARIES_TABLE)
            .execute()
            .await?;
        let batches = table
            .query()
            .full_text_search(FullTextSearchQuery::new(query.to_owned()))
            .only_if(format!("project_id = '{}'", escape_sql_string(project_id)))
            .select(Select::Columns(transcript_summary_result_columns()))
            .limit(limit)
            .execute()
            .await?
            .try_collect::<Vec<_>>()
            .await?;

        transcript_summary_results_from_batches(&batches)
    }

    /// Search transcript summaries by sparse-vector dot product inside a project.
    pub async fn search_lance_transcript_summaries_by_sparse(
        &self,
        project_id: &str,
        sparse_indices: &[u32],
        sparse_values: &[f32],
        limit: usize,
    ) -> Result<Vec<LanceTranscriptSummaryResult>> {
        ensure!(
            sparse_indices.len() == sparse_values.len(),
            "query sparse indices length {} does not match values length {}",
            sparse_indices.len(),
            sparse_values.len()
        );
        if limit == 0 || sparse_indices.is_empty() {
            return Ok(Vec::new());
        }

        let table = self
            .lancedb
            .open_table(TRANSCRIPT_SUMMARIES_TABLE)
            .execute()
            .await?;
        let batches = table
            .query()
            .select(Select::Columns(transcript_summary_sparse_columns()))
            .only_if(format!("project_id = '{}'", escape_sql_string(project_id)))
            .execute()
            .await?
            .try_collect::<Vec<_>>()
            .await?;

        let mut scored = scored_transcript_summary_results_from_batches(
            &batches,
            sparse_indices,
            sparse_values,
        )?;
        scored.sort_by(|a, b| b.0.total_cmp(&a.0));
        Ok(scored
            .into_iter()
            .filter(|(score, _)| *score > 0.0)
            .take(limit)
            .map(|(_, result)| result)
            .collect())
    }

    /// Search transcript summaries with dense vector + sparse vector + full-text retrieval.
    ///
    /// Vector results are kept first because they represent semantic similarity;
    /// full-text results then fill gaps. Duplicate Lance row IDs are removed.
    pub async fn search_lance_transcript_summaries_hybrid(
        &self,
        project_id: &str,
        dense_vector: &[f32],
        sparse_indices: &[u32],
        sparse_values: &[f32],
        text_query: &str,
        limit: usize,
    ) -> Result<Vec<LanceTranscriptSummaryResult>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let vector_results = self
            .search_lance_transcript_summaries_by_vector(project_id, dense_vector, limit)
            .await?;
        let text_results = self
            .search_lance_transcript_summaries_by_text(project_id, text_query, limit)
            .await?;
        let sparse_results = self
            .search_lance_transcript_summaries_by_sparse(
                project_id,
                sparse_indices,
                sparse_values,
                limit,
            )
            .await?;

        Ok(merge_transcript_summary_results(
            vector_results,
            sparse_results,
            text_results,
            limit,
        ))
    }
}

fn merge_transcript_summary_results(
    vector_results: Vec<LanceTranscriptSummaryResult>,
    sparse_results: Vec<LanceTranscriptSummaryResult>,
    text_results: Vec<LanceTranscriptSummaryResult>,
    limit: usize,
) -> Vec<LanceTranscriptSummaryResult> {
    let mut seen = HashSet::new();
    let mut merged = Vec::new();

    for result in vector_results
        .into_iter()
        .chain(sparse_results)
        .chain(text_results)
    {
        if seen.insert(result.id.clone()) {
            merged.push(result);
        }

        if merged.len() == limit {
            break;
        }
    }

    merged
}

fn transcript_summary_sparse_columns() -> Vec<String> {
    let mut columns = transcript_summary_result_columns();
    columns.push("sparse_indices".to_string());
    columns.push("sparse_values".to_string());
    columns
}

fn transcript_summary_result_columns() -> Vec<String> {
    [
        "id",
        "project_id",
        "session_id",
        "summary_index",
        "chunk_start_index",
        "chunk_end_index",
        "start_ms",
        "end_ms",
        "duration_ms",
        "summary",
        "created_at",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn transcript_summary_results_from_batches(
    batches: &[RecordBatch],
) -> Result<Vec<LanceTranscriptSummaryResult>> {
    let mut results = Vec::new();

    for batch in batches {
        let ids = string_column(batch, "id")?;
        let project_ids = string_column(batch, "project_id")?;
        let session_ids = string_column(batch, "session_id")?;
        let summary_indexes = int64_column(batch, "summary_index")?;
        let chunk_start_indexes = int64_column(batch, "chunk_start_index")?;
        let chunk_end_indexes = int64_column(batch, "chunk_end_index")?;
        let start_ms = int64_column(batch, "start_ms")?;
        let end_ms = int64_column(batch, "end_ms")?;
        let duration_ms = int64_column(batch, "duration_ms")?;
        let summaries = string_column(batch, "summary")?;
        let created_ats = int64_column(batch, "created_at")?;

        for row in 0..batch.num_rows() {
            results.push(LanceTranscriptSummaryResult {
                id: ids.value(row).to_owned(),
                project_id: project_ids.value(row).to_owned(),
                session_id: session_ids.value(row).to_owned(),
                summary_index: summary_indexes.value(row),
                chunk_start_index: chunk_start_indexes.value(row),
                chunk_end_index: chunk_end_indexes.value(row),
                start_ms: start_ms.value(row),
                end_ms: end_ms.value(row),
                duration_ms: duration_ms.value(row),
                summary: summaries.value(row).to_owned(),
                created_at_unix_secs: created_ats.value(row),
            });
        }
    }

    Ok(results)
}

fn scored_transcript_summary_results_from_batches(
    batches: &[RecordBatch],
    query_indices: &[u32],
    query_values: &[f32],
) -> Result<Vec<(f32, LanceTranscriptSummaryResult)>> {
    let mut results = Vec::new();

    for batch in batches {
        let base = transcript_summary_results_from_batches(std::slice::from_ref(batch))?;
        let sparse_indices = list_column(batch, "sparse_indices")?;
        let sparse_values = list_column(batch, "sparse_values")?;

        for (row, result) in base.into_iter().enumerate() {
            let indices = sparse_indices
                .value(row)
                .as_any()
                .downcast_ref::<UInt32Array>()
                .context("LanceDB sparse_indices values are not UInt32")?
                .values()
                .to_vec();
            let values = sparse_values
                .value(row)
                .as_any()
                .downcast_ref::<Float32Array>()
                .context("LanceDB sparse_values values are not Float32")?
                .values()
                .to_vec();
            results.push((
                sparse_dot(query_indices, query_values, &indices, &values),
                result,
            ));
        }
    }

    Ok(results)
}

fn sparse_dot(
    left_indices: &[u32],
    left_values: &[f32],
    right_indices: &[u32],
    right_values: &[f32],
) -> f32 {
    let mut score = 0.0;
    for (left_index, left_value) in left_indices.iter().zip(left_values) {
        for (right_index, right_value) in right_indices.iter().zip(right_values) {
            if left_index == right_index {
                score += left_value * right_value;
            }
        }
    }
    score
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

fn list_column<'a>(batch: &'a RecordBatch, name: &str) -> Result<&'a ListArray> {
    batch
        .column_by_name(name)
        .with_context(|| format!("missing LanceDB list column: {name}"))?
        .as_any()
        .downcast_ref::<ListArray>()
        .with_context(|| format!("LanceDB column is not List: {name}"))
}

fn transcript_summary_batch(
    row: LanceTranscriptSummary,
    embedding_dimension: i32,
) -> Result<RecordBatch> {
    let schema = transcript_summary_schema(embedding_dimension);
    let dense_vector = row.dense_vector.into_iter().map(Some).collect::<Vec<_>>();
    let sparse_indices = row.sparse_indices.into_iter().map(Some).collect::<Vec<_>>();
    let sparse_values = row.sparse_values.into_iter().map(Some).collect::<Vec<_>>();

    Ok(RecordBatch::try_new(
        schema,
        vec![
            Arc::new(StringArray::from(vec![row.id])),
            Arc::new(StringArray::from(vec![row.project_id])),
            Arc::new(StringArray::from(vec![row.session_id])),
            Arc::new(Int64Array::from(vec![row.summary_index])),
            Arc::new(Int64Array::from(vec![row.chunk_start_index])),
            Arc::new(Int64Array::from(vec![row.chunk_end_index])),
            Arc::new(Int64Array::from(vec![row.start_ms])),
            Arc::new(Int64Array::from(vec![row.end_ms])),
            Arc::new(Int64Array::from(vec![row.duration_ms])),
            Arc::new(StringArray::from(vec![row.summary])),
            Arc::new(
                FixedSizeListArray::from_iter_primitive::<Float32Type, _, _>(
                    [Some(dense_vector)],
                    embedding_dimension,
                ),
            ),
            Arc::new(ListArray::from_iter_primitive::<UInt32Type, _, _>([Some(
                sparse_indices,
            )])),
            Arc::new(ListArray::from_iter_primitive::<Float32Type, _, _>([Some(
                sparse_values,
            )])),
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
    async fn transcript_summary_can_be_added_searched_and_deleted() {
        let db = connect("memory://").execute().await.unwrap();
        let table = db
            .create_empty_table(TRANSCRIPT_SUMMARIES_TABLE, transcript_summary_schema(DIM))
            .execute()
            .await
            .unwrap();

        table
            .add(
                transcript_summary_batch(
                    LanceTranscriptSummary {
                        id: "summary-1".to_string(),
                        project_id: "project-1".to_string(),
                        session_id: "session-1".to_string(),
                        summary_index: 0,
                        chunk_start_index: 0,
                        chunk_end_index: 3,
                        start_ms: 0,
                        end_ms: 300_000,
                        duration_ms: 300_000,
                        summary: "discussion about rust ownership".to_string(),
                        dense_vector: vec![1.0, 0.0, 0.0, 0.0],
                        sparse_indices: vec![42],
                        sparse_values: vec![0.7],
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
            .create_index(&["summary"], Index::FTS(Default::default()))
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
        let vector_results = transcript_summary_results_from_batches(&vector_batches).unwrap();
        assert_eq!(vector_results[0].id, "summary-1");
        assert_eq!(vector_results[0].summary, "discussion about rust ownership");

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
        let text_results = transcript_summary_results_from_batches(&text_batches).unwrap();
        assert_eq!(text_results[0].session_id, "session-1");

        let merged = merge_transcript_summary_results(vector_results, Vec::new(), text_results, 10);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].id, "summary-1");

        table.delete("id = 'summary-1'").await.unwrap();

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

    #[test]
    fn sparse_dot_scores_matching_dimensions() {
        let score = sparse_dot(&[7, 9], &[2.0, 3.0], &[4, 9], &[8.0, 5.0]);

        assert_eq!(score, 15.0);
    }

    #[test]
    fn hybrid_merge_dedupes_vector_sparse_and_text_hits_in_order() {
        fn hit(id: &str) -> LanceTranscriptSummaryResult {
            LanceTranscriptSummaryResult {
                id: id.to_string(),
                project_id: "project".to_string(),
                session_id: "session".to_string(),
                summary_index: 0,
                chunk_start_index: 0,
                chunk_end_index: 1,
                start_ms: 0,
                end_ms: 1,
                duration_ms: 1,
                summary: id.to_string(),
                created_at_unix_secs: 0,
            }
        }

        let merged = merge_transcript_summary_results(
            vec![hit("dense"), hit("shared")],
            vec![hit("sparse"), hit("shared")],
            vec![hit("text"), hit("dense")],
            10,
        );

        assert_eq!(
            merged.into_iter().map(|hit| hit.id).collect::<Vec<_>>(),
            ["dense", "shared", "sparse", "text"]
        );
    }
}
