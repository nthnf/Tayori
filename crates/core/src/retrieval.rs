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
    let mut sparse_vectors = embedder.sparse_embed_vectors(vec![query.to_string()])?;
    let sparse_vector = sparse_vectors.pop().unwrap_or_default();
    let each_limit = limit.max(2);

    let docs = storage
        .search_lance_document_chunks_hybrid(
            project_id,
            &query_vector,
            &sparse_vector.indices,
            &sparse_vector.values,
            query,
            each_limit,
        )
        .await?;
    let summaries = storage
        .search_lance_transcript_summaries_hybrid(
            project_id,
            &query_vector,
            &sparse_vector.indices,
            &sparse_vector.values,
            query,
            each_limit,
        )
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
        let ranked_indices = indexed
            .into_iter()
            .map(|rank| rank.index)
            .collect::<Vec<_>>();
        hits = apply_ranked_indices(&hits, ranked_indices);
    }

    hits.truncate(limit);
    Ok(hits)
}

fn apply_ranked_indices(hits: &[RetrievalHit], ranked_indices: Vec<usize>) -> Vec<RetrievalHit> {
    ranked_indices
        .into_iter()
        .filter_map(|index| hits.get(index).cloned())
        .collect()
}

pub fn format_context(hits: &[RetrievalHit]) -> String {
    hits.iter()
        .enumerate()
        .map(|(index, hit)| format!("[{}] {}", index + 1, hit.text()))
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use storage::lance_document::LanceDocumentChunkResult;

    fn doc(id: &str, text: &str) -> RetrievalHit {
        RetrievalHit::Document(LanceDocumentChunkResult {
            id: id.to_string(),
            project_id: "project".to_string(),
            document_id: "doc".to_string(),
            chunk_index: 0,
            text: text.to_string(),
            created_at_unix_secs: 0,
        })
    }

    #[test]
    fn rerank_indices_reorder_hits_and_skip_invalid_indices() {
        let hits = vec![doc("a", "first"), doc("b", "second"), doc("c", "third")];
        let reranked = apply_ranked_indices(&hits, vec![2, 99, 0]);

        assert_eq!(reranked, vec![doc("c", "third"), doc("a", "first")]);
    }

    #[test]
    fn format_context_numbers_hits_in_order() {
        let context = format_context(&[doc("a", "alpha"), doc("b", "beta")]);

        assert_eq!(context, "[1] alpha\n\n[2] beta");
    }
}
