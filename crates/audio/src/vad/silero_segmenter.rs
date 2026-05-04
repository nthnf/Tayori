use std::collections::VecDeque;

use anyhow::{Context, Result, bail};
use silero_vad_rust::{
    load_silero_vad,
    silero_vad::utils_vad::{VadEvent, VadIterator, VadIteratorParams},
};
use tracing::{debug, info};

use crate::AudioFrame;

use super::{config::SileroVadConfig, segment::SpeechSegment};

pub struct SileroVadSegmenter {
    iterator: VadIterator,
    config: SileroVadConfig,

    sample_rate: u32,
    in_speech: bool,

    /// Current speech segment being accumulated.
    current: Vec<f32>,

    /// Small rolling buffer before speech starts.
    ///
    /// This helps avoid cutting off the start of a sentence.
    pre_roll: VecDeque<Vec<f32>>,

    frames_seen: u64,
    segments_seen: u64,
}

impl SileroVadSegmenter {
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
            config,
            sample_rate: 16_000,
            in_speech: false,
            current: Vec::new(),
            pre_roll: VecDeque::new(),
            frames_seen: 0,
            segments_seen: 0,
        })
    }

    pub fn with_default_config() -> Result<Self> {
        Self::new(SileroVadConfig::default())
    }

    /// Push one AudioFrame.
    ///
    /// Expected input:
    /// - mono
    /// - 16 kHz
    /// - 512 samples / 32ms
    pub fn push_frame(&mut self, frame: AudioFrame) -> Result<Option<SpeechSegment>> {
        self.validate_frame(&frame)?;

        self.frames_seen += 1;

        let event = self
            .iterator
            .process_chunk(&frame.samples, true, 1)
            .context("Silero VAD failed to process audio frame")?;

        debug!(
            frames_seen = self.frames_seen,
            event = ?event,
            "Silero VAD frame processed"
        );

        match event {
            Some(VadEvent::Start(ts)) => {
                debug!(timestamp = ts, "speech started");

                self.in_speech = true;

                // Keep a little audio before the detected start.
                while let Some(previous) = self.pre_roll.pop_front() {
                    self.current.extend_from_slice(&previous);
                }

                self.current.extend_from_slice(&frame.samples);

                self.maybe_force_emit()
            }

            Some(VadEvent::End(ts)) => {
                debug!(timestamp = ts, "speech ended");

                if self.in_speech {
                    self.current.extend_from_slice(&frame.samples);
                }

                self.in_speech = false;

                self.emit_if_long_enough()
            }

            None => {
                if self.in_speech {
                    self.current.extend_from_slice(&frame.samples);

                    self.maybe_force_emit()
                } else {
                    self.push_pre_roll(frame.samples);
                    Ok(None)
                }
            }
        }
    }

    /// Flush any currently buffered speech.
    ///
    /// Call this when stopping capture.
    pub fn flush(&mut self) -> Option<SpeechSegment> {
        self.in_speech = false;

        let min_samples = self.config.min_segment_samples(self.sample_rate);

        if self.current.len() < min_samples {
            self.current.clear();
            self.pre_roll.clear();
            return None;
        }

        let samples = std::mem::take(&mut self.current);
        self.pre_roll.clear();

        self.segments_seen += 1;

        Some(SpeechSegment::new(samples, self.sample_rate))
    }

    pub fn reset(&mut self) {
        self.iterator.reset_states();
        self.in_speech = false;
        self.current.clear();
        self.pre_roll.clear();
        self.frames_seen = 0;
        self.segments_seen = 0;
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

    fn push_pre_roll(&mut self, samples: Vec<f32>) {
        self.pre_roll.push_back(samples);

        while self.pre_roll.len() > self.config.pre_roll_frames {
            self.pre_roll.pop_front();
        }
    }

    fn maybe_force_emit(&mut self) -> Result<Option<SpeechSegment>> {
        if !self.config.force_emit_on_max_duration {
            return Ok(None);
        }

        let max_samples = self.config.max_segment_samples(self.sample_rate);

        if self.current.len() < max_samples {
            return Ok(None);
        }

        info!(
            samples = self.current.len(),
            "forcing speech segment emit because max duration was reached"
        );

        self.in_speech = false;

        Ok(self.emit_any_current())
    }

    fn emit_if_long_enough(&mut self) -> Result<Option<SpeechSegment>> {
        let min_samples = self.config.min_segment_samples(self.sample_rate);

        if self.current.len() < min_samples {
            debug!(
                samples = self.current.len(),
                min_samples, "discarding too-short speech segment"
            );

            self.current.clear();
            self.pre_roll.clear();

            return Ok(None);
        }

        Ok(self.emit_any_current())
    }

    fn emit_any_current(&mut self) -> Option<SpeechSegment> {
        if self.current.is_empty() {
            return None;
        }

        let samples = std::mem::take(&mut self.current);
        self.pre_roll.clear();

        self.segments_seen += 1;

        let segment = SpeechSegment::new(samples, self.sample_rate);

        info!(
            segments_seen = self.segments_seen,
            duration_seconds = segment.duration_seconds(),
            "speech segment finalized"
        );

        Some(segment)
    }
}
