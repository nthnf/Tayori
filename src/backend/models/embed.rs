use anyhow::Result;
use fastembed::{Embedding, EmbeddingModel, InitOptions, TextEmbedding};
use ort::environment::GlobalThreadPoolOptions;

use std::sync::Mutex;

pub const MODEL: EmbeddingModel = EmbeddingModel::AllMiniLML6V2;

pub struct Embedder {
    model: Mutex<TextEmbedding>,
}

impl Embedder {
    pub fn new() -> Result<Self> {
        // Limit ONNX Runtime threads to prevent CPU spikes when embedding.
        // We ignore the result because ort can only be initialized once globally.
        if !ort::init()
            .with_name("tayori-embedder")
            .with_global_thread_pool(
                GlobalThreadPoolOptions::default()
                    .with_intra_threads(1)
                    .unwrap_or_default()
                    .with_inter_threads(1)
                    .unwrap_or_default(),
            )
            .commit()
        {
            tracing::debug!("ort already initialized globally");
        }

        let model = TextEmbedding::try_new(
            InitOptions::new(EmbeddingModel::AllMiniLML6V2).with_show_download_progress(true),
        )?;

        Ok(Self {
            model: Mutex::new(model),
        })
    }

    pub fn embed(&self, texts: Vec<String>) -> Result<Vec<Embedding>> {
        let mut model = self.model.lock().unwrap_or_else(|e| e.into_inner());
        model.embed(texts, None)
    }
}

pub fn get_model_dim() -> Result<usize> {
    Ok(TextEmbedding::get_model_info(&MODEL)?.dim)
}
