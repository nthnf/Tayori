use anyhow::{Context, Result, bail};
use silero_vad_rust::{
    load_silero_vad,
    silero_vad::utils_vad::{VadEvent, VadIterator, VadIteratorParams},
};

use crate::AudioFrame;

use super::{
    config::SileroVadConfig,
    state::{VadFrameUpdate, VadSignal, VadStateTracker},
};

pub struct SileroVadWatcher {
    iterator: VadIterator,
    tracker: VadStateTracker,
    config: SileroVadConfig,
    sample_rate: u32,
}

impl SileroVadWatcher {
    pub fn new(config: SileroVadConfig) -> Result<Self> {
        let model = load_silero_vad().context("failed to load Silero VAD model")?;

        let params = VadIteratorParams {
            threshold: config.threshold,
            ..Default::default()
        };

        let iterator =
            VadIterator::new(model, params).context("failed to create Silero VAD iterator")?;

        Ok(Self {
            iterator,
            tracker: VadStateTracker::new(),
            config,
            sample_rate: 16_000,
        })
    }

    pub fn with_default_config() -> Result<Self> {
        Self::new(SileroVadConfig::default())
    }

    pub fn push_frame(&mut self, frame: AudioFrame) -> Result<VadFrameUpdate> {
        self.validate_frame(&frame)?;

        let event = self
            .iterator
            .process_chunk(&frame.samples, true, 1)
            .context("Silero VAD failed to process audio frame")?;

        let signal = match event {
            Some(VadEvent::Start(_)) => VadSignal::SpeechStart,
            Some(VadEvent::End(_)) => VadSignal::SpeechEnd,
            None => VadSignal::None,
        };

        Ok(self.tracker.push_frame(frame.samples.len(), signal))
    }

    pub fn reset(&mut self) {
        self.iterator.reset_states();
        self.tracker = VadStateTracker::new();
    }

    fn validate_frame(&self, frame: &AudioFrame) -> Result<()> {
        if frame.sample_rate != self.sample_rate {
            bail!(
                "Silero VAD expects {} Hz, got {} Hz",
                self.sample_rate,
                frame.sample_rate
            );
        }

        if frame.channels != 1 {
            bail!(
                "Silero VAD expects mono audio, got {} channels",
                frame.channels
            );
        }

        if frame.samples.len() != self.config.frame_samples {
            bail!(
                "Silero VAD expects {} samples per frame, got {}",
                self.config.frame_samples,
                frame.samples.len()
            );
        }

        Ok(())
    }
}
