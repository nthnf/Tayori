use std::time::Instant;

use anyhow::{Context, Result, bail};
use tracing::{debug, info, warn};
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

        info!(
            model_path = %config.model_path.display(),
            use_gpu = config.use_gpu,
            gpu_device = config.gpu_device,
            flash_attn = config.flash_attn,
            n_threads = config.n_threads,
            single_segment = config.single_segment,
            "loading Whisper model"
        );

        let load_started = Instant::now();

        let ctx =
            WhisperContext::new_with_params(&config.model_path, ctx_params).with_context(|| {
                format!(
                    "failed to load Whisper model: {}",
                    config.model_path.display()
                )
            })?;

        let load_secs = load_started.elapsed().as_secs_f32();

        let model_type = ctx
            .model_type_readable_str_lossy()
            .map(|value| value.into_owned())
            .unwrap_or_else(|err| format!("unknown ({err})"));

        info!(
            model_path = %config.model_path.display(),
            model_type = %model_type,
            multilingual = ctx.is_multilingual(),
            load_secs,
            "Whisper model loaded"
        );

        Ok(Self { config, ctx })
    }

    pub fn transcribe_samples(&self, samples: &[f32]) -> Result<Transcription> {
        let total_started = Instant::now();

        if samples.is_empty() {
            debug!("skipping empty Whisper transcription request");

            return Ok(Transcription {
                text: String::new(),
                segments: Vec::new(),
            });
        }

        let audio_secs = samples.len() as f32 / 16_000.0;

        debug!(
            samples = samples.len(),
            audio_secs, "Whisper transcription requested"
        );

        let create_state_started = Instant::now();

        let mut state = self
            .ctx
            .create_state()
            .context("failed to create Whisper state")?;

        let create_state_secs = create_state_started.elapsed().as_secs_f32();

        if create_state_secs > 0.050 {
            warn!(
                create_state_secs,
                "Whisper create_state took longer than expected"
            );
        } else {
            debug!(create_state_secs, "Whisper create_state completed");
        }

        let build_params_started = Instant::now();

        let params = self.build_params();

        let build_params_secs = build_params_started.elapsed().as_secs_f32();

        let full_started = Instant::now();

        state
            .full(params, samples)
            .context("failed to run Whisper transcription")?;

        let full_secs = full_started.elapsed().as_secs_f32();

        let collect_started = Instant::now();

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

        let collect_secs = collect_started.elapsed().as_secs_f32();
        let total_secs = total_started.elapsed().as_secs_f32();

        let rtf = if audio_secs > 0.0 {
            total_secs / audio_secs
        } else {
            0.0
        };

        info!(
            samples = samples.len(),
            audio_secs,
            total_secs,
            rtf,
            create_state_secs,
            build_params_secs,
            full_secs,
            collect_secs,
            segments = segments.len(),
            text_len = text.len(),
            "Whisper transcription timing"
        );

        if total_secs > 1.0 {
            warn!(
                audio_secs,
                total_secs,
                rtf,
                create_state_secs,
                full_secs,
                collect_secs,
                "Whisper transcription exceeded 1s"
            );
        }

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
