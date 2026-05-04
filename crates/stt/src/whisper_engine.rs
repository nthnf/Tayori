use anyhow::{Context, Result, bail};
use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, install_logging_hooks,
};

use crate::{SttConfig, SttSampling, TranscriptSegment, Transcription};

pub struct WhisperEngine {
    config: SttConfig,
    ctx: WhisperContext,
}

impl WhisperEngine {
    pub fn new(config: SttConfig) -> Result<Self> {
        if !config.model_path.exists() {
            bail!(
                "Whisper model not found: {}. Run your model installer script first.",
                config.model_path.display()
            );
        }

        // Important:
        // Without log_backend/tracing_backend enabled, this suppresses whisper.cpp/GGML logs.
        install_logging_hooks();

        let mut ctx_params = WhisperContextParameters::default();

        ctx_params.use_gpu(config.use_gpu);
        ctx_params.gpu_device(config.gpu_device);
        ctx_params.flash_attn(config.flash_attn);

        let ctx =
            WhisperContext::new_with_params(&config.model_path, ctx_params).with_context(|| {
                format!(
                    "failed to load Whisper model: {}",
                    config.model_path.display()
                )
            })?;

        Ok(Self { config, ctx })
    }

    pub fn transcribe_samples(&self, samples: &[f32]) -> Result<Transcription> {
        if samples.is_empty() {
            return Ok(Transcription {
                text: String::new(),
                segments: Vec::new(),
            });
        }

        let mut state = self
            .ctx
            .create_state()
            .context("failed to create Whisper state")?;

        let params = self.build_params();

        state
            .full(params, samples)
            .context("failed to run Whisper transcription")?;

        let mut segments = Vec::new();

        for segment in state.as_iter() {
            let text = segment.to_string().trim().to_string();

            if text.is_empty() {
                continue;
            }

            segments.push(TranscriptSegment {
                start_cs: segment.start_timestamp(),
                end_cs: segment.end_timestamp(),
                text,
            });
        }

        let text = segments
            .iter()
            .map(|segment| segment.text.as_str())
            .collect::<Vec<_>>()
            .join(" ")
            .trim()
            .to_string();

        Ok(Transcription { text, segments })
    }

    fn build_params(&self) -> FullParams<'_, '_> {
        let sampling = match self.config.sampling {
            SttSampling::Greedy { best_of } => SamplingStrategy::Greedy { best_of },
            SttSampling::BeamSearch {
                beam_size,
                patience,
            } => SamplingStrategy::BeamSearch {
                beam_size,
                patience,
            },
        };

        let mut params = FullParams::new(sampling);

        params.set_n_threads(self.config.n_threads);
        params.set_translate(false);
        params.set_no_context(true);
        params.set_single_segment(self.config.single_segment);

        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_token_timestamps(false);

        params.set_language(self.config.language.as_whisper_language());

        params
    }
}
