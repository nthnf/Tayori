use sea_orm::{DatabaseConnection, DbBackend, FromQueryResult, Statement, Value};
use std::collections::HashMap;

#[derive(FromQueryResult, Clone, Debug)]
pub struct SearchCandidate {
    pub id: String,
    pub source_type: String,
    pub content: String,
    pub raw_score: f64,
}

/// The 'Hybrid Funnel': FTS + Vector + RRF
pub async fn smart_hybrid_search(
    db: &DatabaseConnection,
    query_text: &str,
    query_vector: Vec<f32>, // Using the binary f32 blob for sqlite-vector
    limit: Option<usize>,
) -> Result<(f64, f64, Vec<SearchCandidate>), sea_orm::DbErr> {
    let k = 60.0; // RRF constant
    let limit = limit.unwrap_or(10);

    // 1. EXECUTE FTS5 SEARCH (Keyword)
    // Sanitize query for FTS5 (remove punctuation that breaks MATCH syntax like '?')
    let fts_query = query_text.replace(|c: char| !c.is_alphanumeric() && !c.is_whitespace(), "");
    let fts_stmt = Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT source_id as id, source_type, content, abs(bm25(global_fts)) as raw_score FROM global_fts WHERE content MATCH $1 ORDER BY bm25(global_fts) LIMIT 20",
        vec![fts_query.into()],
    );
    let fts_results = SearchCandidate::find_by_statement(fts_stmt).all(db).await?;

    // 2. EXECUTE VECTOR SEARCH (Semantic)
    let document_vector_stmt = Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT e.id, 'document' as source_type, e.content, (1.0 - v.distance) as raw_score FROM document_chunks AS e
         JOIN vector_full_scan('document_chunks', 'vector', $1, 20) AS v
         ON e.rowid = v.rowid",
        vec![vector_to_values(&query_vector)],
    );
    let document_vector_results = SearchCandidate::find_by_statement(document_vector_stmt)
        .all(db)
        .await?;

    let summary_vector_stmt = Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT e.id, 'summary' as source_type, e.summary as content, (1.0 - v.distance) as raw_score FROM transcript_summaries AS e
         JOIN vector_full_scan('transcript_summaries', 'vector', $1, 20) AS v
         ON e.rowid = v.rowid",
        vec![vector_to_values(&query_vector)],
    );
    let summary_vector_results = SearchCandidate::find_by_statement(summary_vector_stmt)
        .all(db)
        .await?;

    // Track Maximums
    let mut max_bm25: f64 = 0.0;
    let mut max_cosine: f64 = 0.0;

    for item in &fts_results {
        if item.raw_score > max_bm25 {
            max_bm25 = item.raw_score;
        }
    }
    for item in &document_vector_results {
        if item.raw_score > max_cosine {
            max_cosine = item.raw_score;
        }
    }
    for item in &summary_vector_results {
        if item.raw_score > max_cosine {
            max_cosine = item.raw_score;
        }
    }

    // 3. RRF MERGE & DEDUPE
    let mut score_map: HashMap<(String, String), f64> = HashMap::new();
    let mut item_map: HashMap<(String, String), SearchCandidate> = HashMap::new();

    for (rank, item) in fts_results.into_iter().enumerate() {
        let score = 1.0 / (k + (rank + 1) as f64);
        let key = (item.source_type.clone(), item.id.clone());
        score_map.insert(key.clone(), score);
        item_map.insert(key, item);
    }

    for (rank, item) in document_vector_results.into_iter().enumerate() {
        let score = 1.0 / (k + (rank + 1) as f64);
        let key = (item.source_type.clone(), item.id.clone());
        *score_map.entry(key.clone()).or_insert(0.0) += score;
        item_map.entry(key).or_insert(item);
    }

    for (rank, item) in summary_vector_results.into_iter().enumerate() {
        let score = 1.0 / (k + (rank + 1) as f64);
        let key = (item.source_type.clone(), item.id.clone());
        *score_map.entry(key.clone()).or_insert(0.0) += score;
        item_map.entry(key).or_insert(item);
    }

    // 4. FINAL SORT
    let mut final_list: Vec<((String, String), f64)> = score_map.into_iter().collect();
    final_list.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let top_candidates = final_list
        .into_iter()
        .take(limit)
        .filter_map(|(key, _)| item_map.remove(&key))
        .collect();

    Ok((max_cosine, max_bm25, top_candidates))
}

/// Converts a slice of f32 into a SeaORM-compatible binary Value for sqlite-vector
fn vector_to_values(vec: &[f32]) -> Value {
    // 1. Convert f32s to little-endian bytes
    let mut bytes = Vec::with_capacity(vec.len() * 4);
    for &f in vec {
        bytes.extend_from_slice(&f.to_le_bytes());
    }

    // 2. Wrap the Vec<u8> in a SeaORM Value::Bytes
    // We use Some(...) because it's a non-null BLOB
    Value::Bytes(Some(bytes))
}
