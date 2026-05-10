//! Speech-to-text utilities built around Whisper.
//!
//! This crate owns the model path logic, the transcription wrapper, and the
//! background worker that turns raw PCM jobs into transcription chunks.

/// Final transcribed chunk emitted by STT.
pub mod chunk;
/// Raw jobs fed into the transcription worker.
pub mod job;
/// Model path helpers.
pub mod path;
/// Whisper model wrapper.
pub mod transcribe;
/// Background transcription worker.
pub mod worker;
