use anyhow::Result;
use std::time::Instant;
use tracing::debug;
use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, install_logging_hooks,
};

use crate::path::{ensure_path_exists, model_path};

/// Whisper wrapper that transcribes raw mono PCM samples.
pub struct Transcriptor {
    ctx: WhisperContext,
}

impl Transcriptor {
    /// Load a whisper model from disk into a reusable context.
    pub fn new(model_name: &str) -> Result<Self> {
        // Route whisper.cpp C logs through Rust logging hooks when supported.
        // Without this, some backend logs print directly to stderr.
        install_logging_hooks();

        // Context owns the loaded model and GPU/CPU backend resources. It is the
        // expensive reusable part of Whisper, so keep it on the Transcriptor.
        let path = model_path(model_name);
        ensure_path_exists(&path)?;
        let ctx = WhisperContext::new_with_params(path, WhisperContextParameters::default())?;

        Ok(Self { ctx })
    }

    /// Transcribe raw 16 kHz mono samples into plain text.
    ///
    /// This creates a fresh whisper state per call, so the wrapper stays free of
    /// lifetime parameters and can be shared by worker threads safely.
    pub fn transcribe(&self, samples: &[f32]) -> Result<String> {
        let started_at = Instant::now();

        // State is per-run scratch space plus output segments. Fresh state keeps
        // each VAD chunk isolated and avoids stale decode results.
        let mut state = self.ctx.create_state()?;
        let state_created_at = Instant::now();

        // Params describe how this run should decode. Greedy is lower latency
        // than beam search and works better for live-ish transcription.
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });

        // Inputs are expected to be English 16 kHz mono f32. Disable whisper.cpp
        // console printing because Tayori handles output/logging itself.
        params.set_language(Some("en"));
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);

        // `full` does mel generation, encode, decode, and segment creation. This
        // is the expensive part measured as inference_ms below.
        state.full(params, samples)?;
        let inference_done_at = Instant::now();

        // Pull generated text out of all returned segments and flatten it into
        // one chunk body for the rest of the app.
        let mut text = String::new();
        for segment in state.as_iter() {
            text.push_str(segment.to_str_lossy()?.as_ref());
        }

        debug!(
            samples = samples.len(),
            audio_ms = samples.len() as f64 / 16_000.0 * 1_000.0,
            state_create_ms = state_created_at.duration_since(started_at).as_secs_f64() * 1_000.0,
            inference_ms = inference_done_at
                .duration_since(state_created_at)
                .as_secs_f64()
                * 1_000.0,
            collect_text_ms = inference_done_at.elapsed().as_secs_f64() * 1_000.0,
            total_ms = started_at.elapsed().as_secs_f64() * 1_000.0,
            text_len = text.len(),
            "Whisper transcription metrics"
        );

        Ok(text)
    }
}
