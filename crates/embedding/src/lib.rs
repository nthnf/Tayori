use anyhow::{Result, bail};
use fastembed::{
    Embedding, EmbeddingModel, InitOptions, RerankInitOptions, RerankResult, RerankerModel,
    SparseEmbedding, SparseInitOptions, SparseModel, SparseTextEmbedding, TextEmbedding,
    TextRerank,
};

/// Sparse vector in the shape storage expects for LanceDB rows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SparseVector {
    pub indices: Vec<u32>,
    pub values: Vec<f32>,
}

impl TryFrom<SparseEmbedding> for SparseVector {
    type Error = anyhow::Error;

    fn try_from(value: SparseEmbedding) -> Result<Self> {
        let indices = value
            .indices
            .into_iter()
            .map(|index| {
                u32::try_from(index)
                    .map_err(|_| anyhow::anyhow!("sparse index does not fit into u32: {index}"))
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(Self {
            indices,
            values: value.values,
        })
    }
}

pub struct Embedder {
    pub dense: TextEmbedding,
    pub sparse: SparseTextEmbedding,
    pub reranker: TextRerank,
    dense_dim: usize,
}

impl Embedder {
    pub fn new(
        dense_model_name: &str,
        sparse_model_name: &str,
        reranker_model_name: &str,
    ) -> Result<Self> {
        let dense_model = Self::dense_str_to_model(dense_model_name)?;
        let sparse_model = Self::sparse_str_to_model(sparse_model_name)?;
        let rerank_model = Self::reranker_str_to_model(reranker_model_name)?;

        let dense_dim = TextEmbedding::get_model_info(&dense_model)?.dim;

        let dense = TextEmbedding::try_new(
            InitOptions::new(dense_model).with_show_download_progress(true),
        )?;

        let sparse = SparseTextEmbedding::try_new(
            SparseInitOptions::new(sparse_model).with_show_download_progress(true),
        )?;

        let reranker = TextRerank::try_new(
            RerankInitOptions::new(rerank_model).with_show_download_progress(true),
        )?;

        Ok(Self {
            dense,
            sparse,
            reranker,
            dense_dim,
        })
    }

    pub fn dense_dim(&self) -> usize {
        self.dense_dim
    }

    pub fn dense_embed(&mut self, texts: Vec<String>) -> Result<Vec<Embedding>> {
        self.dense.embed(texts, None)
    }

    pub fn sparse_embed(&mut self, texts: Vec<String>) -> Result<Vec<SparseEmbedding>> {
        self.sparse.embed(texts, None)
    }

    pub fn sparse_embed_vectors(&mut self, texts: Vec<String>) -> Result<Vec<SparseVector>> {
        self.sparse_embed(texts)?
            .into_iter()
            .map(SparseVector::try_from)
            .collect()
    }

    pub fn rerank(&mut self, query: &str, docs: Vec<String>) -> Result<Vec<RerankResult>> {
        self.reranker.rerank(query.to_owned(), docs, true, None)
    }

    fn dense_str_to_model(model: &str) -> Result<EmbeddingModel> {
        let normalized = model.trim().to_ascii_lowercase();

        match normalized.as_str() {
            // BAAI
            "baai/bge-small-en-v1.5" => Ok(EmbeddingModel::BGESmallENV15),
            "baai/bge-base-en-v1.5" => Ok(EmbeddingModel::BGEBaseENV15),
            "baai/bge-large-en-v1.5" => Ok(EmbeddingModel::BGELargeENV15),
            "baai/bge-small-zh-v1.5" => Ok(EmbeddingModel::BGESmallZHV15),
            "baai/bge-large-zh-v1.5" => Ok(EmbeddingModel::BGELargeZHV15),
            "baai/bge-m3" => Ok(EmbeddingModel::BGEM3),

            // Sentence Transformers
            "sentence-transformers/all-minilm-l6-v2" => Ok(EmbeddingModel::AllMiniLML6V2),
            "sentence-transformers/all-minilm-l12-v2" => Ok(EmbeddingModel::AllMiniLML12V2),
            "sentence-transformers/all-mpnet-base-v2" => Ok(EmbeddingModel::AllMpnetBaseV2),

            // FastEmbed Rust maps this enum to the multilingual MiniLM L12 model internally.
            "sentence-transformers/paraphrase-minilm-l12-v2"
            | "sentence-transformers/paraphrase-multilingual-minilm-l12-v2"
            | "xenova/paraphrase-multilingual-minilm-l12-v2" => {
                Ok(EmbeddingModel::ParaphraseMLMiniLML12V2)
            }

            "sentence-transformers/paraphrase-multilingual-mpnet-base-v2"
            | "xenova/paraphrase-multilingual-mpnet-base-v2" => {
                Ok(EmbeddingModel::ParaphraseMLMpnetBaseV2)
            }

            // Nomic
            "nomic-ai/nomic-embed-text-v1" => Ok(EmbeddingModel::NomicEmbedTextV1),
            "nomic-ai/nomic-embed-text-v1.5" => Ok(EmbeddingModel::NomicEmbedTextV15),

            // E5
            "intfloat/multilingual-e5-small" => Ok(EmbeddingModel::MultilingualE5Small),
            "intfloat/multilingual-e5-base" => Ok(EmbeddingModel::MultilingualE5Base),
            "intfloat/multilingual-e5-large" => Ok(EmbeddingModel::MultilingualE5Large),

            // Mixedbread
            "mixedbread-ai/mxbai-embed-large-v1" => Ok(EmbeddingModel::MxbaiEmbedLargeV1),

            // Alibaba GTE
            "alibaba-nlp/gte-base-en-v1.5" => Ok(EmbeddingModel::GTEBaseENV15),
            "alibaba-nlp/gte-large-en-v1.5" => Ok(EmbeddingModel::GTELargeENV15),

            // ModernBERT
            "lightonai/modernbert-embed-large" => Ok(EmbeddingModel::ModernBertEmbedLarge),

            // CLIP text encoder
            "qdrant/clip-vit-b-32-text" => Ok(EmbeddingModel::ClipVitB32),

            // Jina
            "jinaai/jina-embeddings-v2-base-code" => Ok(EmbeddingModel::JinaEmbeddingsV2BaseCode),
            "jinaai/jina-embeddings-v2-base-en" => Ok(EmbeddingModel::JinaEmbeddingsV2BaseEN),

            // EmbeddingGemma
            // Rust FastEmbed uses the ONNX community model code internally,
            // but this alias lets your UI accept the official-looking Google name too.
            "google/embeddinggemma-300m" => Ok(EmbeddingModel::EmbeddingGemma300M),

            // Snowflake Arctic
            "snowflake/snowflake-arctic-embed-xs" => Ok(EmbeddingModel::SnowflakeArcticEmbedXS),
            "snowflake/snowflake-arctic-embed-s" => Ok(EmbeddingModel::SnowflakeArcticEmbedS),
            "snowflake/snowflake-arctic-embed-m" => Ok(EmbeddingModel::SnowflakeArcticEmbedM),
            "snowflake/snowflake-arctic-embed-m-long" => {
                Ok(EmbeddingModel::SnowflakeArcticEmbedMLong)
            }
            "snowflake/snowflake-arctic-embed-l" => Ok(EmbeddingModel::SnowflakeArcticEmbedL),

            _ => bail!("unsupported dense embedding model: {model}"),
        }
    }

    fn sparse_str_to_model(model: &str) -> Result<SparseModel> {
        let normalized = model.trim().to_ascii_lowercase();

        match normalized.as_str() {
            "prithivida/splade_pp_en_v1" => Ok(SparseModel::SPLADEPPV1),
            "baai/bge-m3" => Ok(SparseModel::BGEM3),

            _ => bail!("unsupported sparse embedding model: {model}"),
        }
    }

    fn reranker_str_to_model(model: &str) -> Result<RerankerModel> {
        let normalized = model.trim().to_ascii_lowercase();

        match normalized.as_str() {
            "baai/bge-reranker-base" => Ok(RerankerModel::BGERerankerBase),

            "baai/bge-reranker-v2-m3" | "rozgo/bge-reranker-v2-m3" => {
                Ok(RerankerModel::BGERerankerV2M3)
            }

            "jinaai/jina-reranker-v1-turbo-en" => Ok(RerankerModel::JINARerankerV1TurboEn),

            "jinaai/jina-reranker-v2-base-multilingual"
            | "jinaai/jina-reranker-v2-base-multiligual" => {
                Ok(RerankerModel::JINARerankerV2BaseMultiligual)
            }

            _ => bail!("unsupported reranker model: {model}"),
        }
    }
}
