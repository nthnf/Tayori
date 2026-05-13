use anyhow::Result;
use embedding::Embedder;
use storage::{
    lance_document::LanceDocumentChunkResult, lance_summary::LanceTranscriptSummaryResult,
    storage::Storage,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RetrievalHit {
    Document(LanceDocumentChunkResult),
    Summary(LanceTranscriptSummaryResult),
}

impl RetrievalHit {
    pub fn text(&self) -> &str {
        match self {
            Self::Document(hit) => &hit.text,
            Self::Summary(hit) => &hit.summary,
        }
    }
}

pub async fn retrieve_project_context(
    storage: &Storage,
    embedder: &mut Embedder,
    project_id: &str,
    query: &str,
    limit: usize,
) -> Result<Vec<RetrievalHit>> {
    if limit == 0 {
        return Ok(Vec::new());
    }

    let mut query_vectors = embedder.dense_embed(vec![query.to_string()])?;
    let query_vector = query_vectors.pop().unwrap_or_default();
    let each_limit = limit.max(2);

    let docs = storage
        .search_lance_document_chunks_hybrid(project_id, &query_vector, query, each_limit)
        .await?;
    let summaries = storage
        .search_lance_transcript_summaries_hybrid(project_id, &query_vector, query, each_limit)
        .await?;

    let mut hits = docs
        .into_iter()
        .map(RetrievalHit::Document)
        .chain(summaries.into_iter().map(RetrievalHit::Summary))
        .collect::<Vec<_>>();

    if hits.len() > 1 {
        let docs = hits
            .iter()
            .map(|hit| hit.text().to_string())
            .collect::<Vec<_>>();
        let mut indexed = embedder.rerank(query, docs)?;
        indexed.sort_by(|a, b| b.score.total_cmp(&a.score));
        hits = indexed
            .into_iter()
            .filter_map(|rank| hits.get(rank.index).cloned())
            .collect();
    }

    hits.truncate(limit);
    Ok(hits)
}

pub fn format_context(hits: &[RetrievalHit]) -> String {
    hits.iter()
        .enumerate()
        .map(|(index, hit)| format!("[{}] {}", index + 1, hit.text()))
        .collect::<Vec<_>>()
        .join("\n\n")
}
