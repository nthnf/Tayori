use std::path::PathBuf;

use crate::default_whisper_model_path;

#[derive(Debug, Clone)]
pub struct SttConfig {
    pub model_path: PathBuf,

    /// Use GPU if whisper.cpp backend supports it.
    pub use_gpu: bool,

    /// GPU device index.
    pub gpu_device: i32,

    /// Flash attention can improve speed/memory in supported backends.
    pub flash_attn: bool,

    /// Number of CPU threads used by whisper decoding.
    pub n_threads: i32,

    /// Language behavior.
    pub language: SttLanguage,

    /// Decode strategy.
    pub sampling: SttSampling,

    /// Useful for short live segments.
    pub single_segment: bool,
}

impl Default for SttConfig {
    fn default() -> Self {
        Self {
            model_path: default_whisper_model_path(),
            use_gpu: true,
            gpu_device: 0,
            flash_attn: true,
            n_threads: default_thread_count(),
            language: SttLanguage::English,
            sampling: SttSampling::Greedy { best_of: 1 },
            single_segment: false,
        }
    }
}

#[derive(Debug, Clone)]
pub enum SttLanguage {
    English,
    Auto,
    Other(String),
}

impl SttLanguage {
    pub fn as_whisper_language(&self) -> Option<&str> {
        match self {
            Self::English => Some("en"),
            Self::Auto => None,
            Self::Other(value) => Some(value.as_str()),
        }
    }
}

#[derive(Debug, Clone)]
pub enum SttSampling {
    Greedy { best_of: i32 },
    BeamSearch { beam_size: i32, patience: f32 },
}

fn default_thread_count() -> i32 {
    let count = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);

    count.clamp(1, 8) as i32
}
