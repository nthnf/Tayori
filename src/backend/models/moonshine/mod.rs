pub mod decode;
pub mod features;
pub mod model;
pub mod session;
pub mod streaming;

pub use model::{MoonshineModel, MoonshineParams};
pub use streaming::{MoonshineStreamingParams, StreamingConfig, StreamingModel, StreamingState};

#[derive(Debug, Clone, Default, PartialEq)]
pub enum Quantization {
    #[default]
    FP32,
    FP16,
    Int8,
    Int4,
}

#[derive(Debug, Clone, Default)]
pub struct TranscribeOptions {
    pub language: Option<String>,
    pub translate: bool,
    pub leading_silence_ms: Option<u32>,
    pub trailing_silence_ms: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct TranscriptionResult {
    pub text: String,
    pub segments: Option<Vec<TranscriptionSegment>>,
}

#[derive(Debug, Clone)]
pub struct TranscriptionSegment {
    pub start: f32,
    pub end: f32,
    pub text: String,
}

#[derive(thiserror::Error, Debug)]
pub enum TranscribeError {
    #[error("ONNX Runtime configuration error: {0}")]
    Config(String),
    #[error("Inference failed: {0}")]
    Inference(String),
    #[error("Model file not found: {0}")]
    ModelNotFound(std::path::PathBuf),
    #[error("Audio processing error: {0}")]
    Audio(String),
    #[error("Decoding error: {0}")]
    Decode(String),
}

impl From<ort::Error> for TranscribeError {
    fn from(e: ort::Error) -> Self {
        TranscribeError::Inference(e.to_string())
    }
}

impl From<ndarray::ShapeError> for TranscribeError {
    fn from(e: ndarray::ShapeError) -> Self {
        TranscribeError::Inference(e.to_string())
    }
}

impl From<std::io::Error> for TranscribeError {
    fn from(e: std::io::Error) -> Self {
        TranscribeError::Config(e.to_string())
    }
}

impl From<serde_json::Error> for TranscribeError {
    fn from(e: serde_json::Error) -> Self {
        TranscribeError::Config(e.to_string())
    }
}

pub trait SpeechModel: Send {
    fn transcribe_raw(
        &mut self,
        samples: &[f32],
        options: &TranscribeOptions,
    ) -> Result<TranscriptionResult, TranscribeError>;
}

pub const SAMPLE_RATE: u32 = 16000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MoonshineVariant {
    #[default]
    Tiny,
    TinyAr,
    TinyZh,
    TinyJa,
    TinyKo,
    TinyUk,
    TinyVi,
    Base,
    BaseEs,
}

impl MoonshineVariant {
    pub fn num_layers(&self) -> usize {
        match self {
            MoonshineVariant::Tiny
            | MoonshineVariant::TinyAr
            | MoonshineVariant::TinyZh
            | MoonshineVariant::TinyJa
            | MoonshineVariant::TinyKo
            | MoonshineVariant::TinyUk
            | MoonshineVariant::TinyVi => 6,
            MoonshineVariant::Base | MoonshineVariant::BaseEs => 8,
        }
    }

    pub fn num_key_value_heads(&self) -> usize {
        8
    }

    pub fn head_dim(&self) -> usize {
        match self {
            MoonshineVariant::Tiny
            | MoonshineVariant::TinyAr
            | MoonshineVariant::TinyZh
            | MoonshineVariant::TinyJa
            | MoonshineVariant::TinyKo
            | MoonshineVariant::TinyUk
            | MoonshineVariant::TinyVi => 36,
            MoonshineVariant::Base | MoonshineVariant::BaseEs => 52,
        }
    }

    pub fn token_rate(&self) -> usize {
        match self {
            MoonshineVariant::Tiny | MoonshineVariant::Base | MoonshineVariant::BaseEs => 6,
            MoonshineVariant::TinyUk => 8,
            MoonshineVariant::TinyAr
            | MoonshineVariant::TinyZh
            | MoonshineVariant::TinyJa
            | MoonshineVariant::TinyKo
            | MoonshineVariant::TinyVi => 13,
        }
    }
}
