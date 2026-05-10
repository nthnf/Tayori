use anyhow::{Context, Result};
use crossbeam_channel::Sender;
use ringbuf::{traits::Consumer, HeapCons};
use silero_vad_rust::{
    load_silero_vad,
    silero_vad::utils_vad::{VadEvent, VadIterator, VadIteratorParams},
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

use crate::segment::SpeechSegment;

/// VAD worker configuration.
#[derive(Debug, Clone)]
pub struct VadConfig {
    /// Silero probability threshold for speech detection.
    pub threshold: f32,
    /// Input sample rate. Tayori uses 16 kHz after resample.
    pub sample_rate: u32,
    /// Fixed VAD frame size. 512 samples at 16 kHz ~= 32 ms.
    pub frame_samples: usize,
    /// Maximum speech chunk length before forced split.
    pub max_chunk_ms: u32,
    /// Silero speech pad around start/end in milliseconds.
    pub speech_pad_ms: u32,
    /// Required silence duration before speech end.
    pub min_silence_ms: u32,
}

impl Default for VadConfig {
    fn default() -> Self {
        Self {
            threshold: 0.2,
            sample_rate: 16_000,
            frame_samples: 512,
            max_chunk_ms: 6_000,
            speech_pad_ms: 100,
            min_silence_ms: 250,
        }
    }
}

/// Background worker that turns 16 kHz mono audio into finalized speech segments.
///
/// Pipeline owner provides input ring consumer, output channel, and stop flag.
/// Worker stays idle until `start()` is called.
pub struct VadWorker {
    input: Option<HeapCons<f32>>,
    output: Sender<SpeechSegment>,
    iterator: Option<VadIterator>,
    stop: Arc<AtomicBool>,
    config: VadConfig,
    worker: Option<thread::JoinHandle<()>>,
    segment_index: u64,
}

impl VadWorker {
    /// Create worker from existing input ring and output channel.
    pub fn new(
        config: VadConfig,
        input: HeapCons<f32>,
        output: Sender<SpeechSegment>,
        stop: Arc<AtomicBool>,
    ) -> Result<Self> {
        anyhow::ensure!(
            config.sample_rate == 16_000,
            "VAD worker expects 16 kHz input"
        );
        anyhow::ensure!(config.frame_samples > 0, "frame_samples must be > 0");
        anyhow::ensure!(config.max_chunk_ms > 0, "max_chunk_ms must be > 0");

        let model = load_silero_vad().context("Failed to load silero VAD model")?;

        let params = VadIteratorParams {
            threshold: config.threshold,
            sampling_rate: config.sample_rate,
            speech_pad_ms: config.speech_pad_ms,
            min_silence_duration_ms: config.min_silence_ms,
        };

        let iterator =
            VadIterator::new(model, params).context("Failed to create silero VAD iterator")?;

        Ok(Self {
            input: Some(input),
            output,
            iterator: Some(iterator),
            stop,
            config,
            worker: None,
            segment_index: 0,
        })
    }

    /// Start VAD worker thread.
    pub fn start(&mut self) -> Result<()> {
        anyhow::ensure!(self.worker.is_none(), "VAD worker already started");

        let mut input = self
            .input
            .take()
            .ok_or_else(|| anyhow::anyhow!("VAD input already taken"))?;
        let mut iterator = self
            .iterator
            .take()
            .ok_or_else(|| anyhow::anyhow!("VAD iterator already taken"))?;
        let output = self.output.clone();
        let stop = self.stop.clone();
        let config = self.config.clone();
        let mut segment_index = self.segment_index;

        self.worker = Some(thread::spawn(move || {
            let mut batch = Vec::<f32>::with_capacity(config.frame_samples);
            // Holds audio belonging to the current speech segment. It is flushed
            // on VAD end or when max_chunk_ms forces a live split.
            let mut speech_samples = Vec::<f32>::new();
            // Keeps tail samples that are fewer than one VAD frame so frame
            // alignment stays continuous across worker loop iterations.
            let mut pending_samples = Vec::<f32>::new();
            let mut in_speech = false;
            let mut speech_started_at_sample = 0_u64;
            let mut processed_samples = 0_u64;
            let mut stats = VadStats::new(config.sample_rate, config.frame_samples);
            let max_chunk_samples =
                ((config.sample_rate as usize * config.max_chunk_ms as usize) / 1_000).max(1);

            info!(
                threshold = config.threshold,
                sample_rate = config.sample_rate,
                frame_samples = config.frame_samples,
                min_silence_ms = config.min_silence_ms,
                speech_pad_ms = config.speech_pad_ms,
                max_chunk_ms = config.max_chunk_ms,
                "VAD worker started"
            );

            loop {
                batch.clear();

                // Drain available 16 kHz mono samples from resampler output.
                // VAD processing below only uses complete fixed-size frames.
                while let Some(sample) = input.try_pop() {
                    batch.push(sample);
                }

                if batch.is_empty() {
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }

                    thread::sleep(Duration::from_millis(10));
                    continue;
                }

                pending_samples.extend_from_slice(&batch);

                // Silero expects fixed frame sizes. Keep any partial tail in
                // `pending_samples` for the next pass instead of dropping it.
                let usable_len =
                    pending_samples.len() / config.frame_samples * config.frame_samples;
                if usable_len == 0 {
                    continue;
                }

                for frame in pending_samples[..usable_len].chunks(config.frame_samples) {
                    let frame_started_at = Instant::now();
                    // `process_chunk` returns start/end events when speech state
                    // changes; `None` means continue current state.
                    let event = match iterator.process_chunk(frame, true, 1) {
                        Ok(event) => event,
                        Err(_err) => {
                            warn!("VAD failed to process frame; dropping frame");
                            continue;
                        }
                    };
                    let vad_duration = frame_started_at.elapsed();
                    processed_samples += frame.len() as u64;
                    stats.record_frame(vad_duration);

                    match event {
                        Some(VadEvent::Start(_ts)) => {
                            // Start a new speech segment with the triggering frame.
                            in_speech = true;
                            speech_started_at_sample =
                                processed_samples.saturating_sub(frame.len() as u64);
                            speech_samples.clear();
                            speech_samples.extend_from_slice(frame);
                            info!(
                                segment_index,
                                audio_position_ms =
                                    samples_to_ms(speech_started_at_sample, config.sample_rate),
                                "VAD speech start"
                            );
                            Self::flush_max_chunk(
                                &mut speech_samples,
                                max_chunk_samples,
                                &output,
                                &mut segment_index,
                            );
                        }
                        Some(VadEvent::End(_ts)) => {
                            if in_speech {
                                // Include the end frame, then emit the complete
                                // speech segment for transcription.
                                speech_samples.extend_from_slice(frame);
                                Self::flush_max_chunk(
                                    &mut speech_samples,
                                    max_chunk_samples,
                                    &output,
                                    &mut segment_index,
                                );
                                Self::flush_chunk(
                                    std::mem::take(&mut speech_samples),
                                    &output,
                                    &mut segment_index,
                                );
                                let end_sample = processed_samples;
                                info!(
                                    segment_index = segment_index.saturating_sub(1),
                                    audio_position_ms =
                                        samples_to_ms(end_sample, config.sample_rate),
                                    segment_audio_ms = samples_to_ms(
                                        end_sample.saturating_sub(speech_started_at_sample),
                                        config.sample_rate
                                    ),
                                    "VAD speech end; segment emitted"
                                );
                            }

                            in_speech = false;
                        }
                        None => {
                            if in_speech {
                                // Continue accumulating frames while speech is active.
                                speech_samples.extend_from_slice(frame);
                                Self::flush_max_chunk(
                                    &mut speech_samples,
                                    max_chunk_samples,
                                    &output,
                                    &mut segment_index,
                                );
                            }
                        }
                    }
                }

                pending_samples.drain(..usable_len);
                stats.maybe_report();

                if stop.load(Ordering::Relaxed) {
                    break;
                }
            }

            if in_speech {
                // Shutdown during speech should still emit what we heard so far.
                speech_samples.extend_from_slice(&pending_samples);
                Self::flush_chunk(speech_samples, &output, &mut segment_index);
                info!(
                    segment_index = segment_index.saturating_sub(1),
                    "VAD stopped during speech; final segment emitted"
                );
            }
        }));

        Ok(())
    }

    /// Stop VAD worker thread.
    pub fn stop(&mut self) {
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }

    /// Build finalized speech segment.
    fn create_chunk(samples: Vec<f32>, index: u64) -> SpeechSegment {
        SpeechSegment::new(samples, index)
    }

    /// Force out a final chunk and increment the segment index.
    fn flush_chunk(samples: Vec<f32>, output: &Sender<SpeechSegment>, segment_index: &mut u64) {
        if samples.is_empty() {
            return;
        }

        let segment = Self::create_chunk(samples, *segment_index);
        debug!(
            segment_index = *segment_index,
            samples = segment.samples.len(),
            "VAD segment queued"
        );
        *segment_index += 1;
        let _ = output.send(segment);
    }

    /// Split the live buffer into fixed-size chunks when it grows too large.
    fn flush_max_chunk(
        speech_samples: &mut Vec<f32>,
        max_chunk_samples: usize,
        output: &Sender<SpeechSegment>,
        segment_index: &mut u64,
    ) {
        while speech_samples.len() >= max_chunk_samples {
            let mut chunk = Vec::new();
            std::mem::swap(&mut chunk, speech_samples);
            // Keep overflow after max chunk so no speech samples are lost.
            let overflow = if chunk.len() > max_chunk_samples {
                chunk.split_off(max_chunk_samples)
            } else {
                Vec::new()
            };

            Self::flush_chunk(chunk, output, segment_index);
            *speech_samples = overflow;
        }
    }
}

fn samples_to_ms(samples: u64, sample_rate: u32) -> f64 {
    if sample_rate == 0 {
        return 0.0;
    }

    samples as f64 / sample_rate as f64 * 1_000.0
}

struct VadStats {
    sample_rate: u32,
    frame_samples: usize,
    last_report: Instant,
    frames: u64,
    max_frame_duration: Duration,
    total_frame_duration: Duration,
}

impl VadStats {
    fn new(sample_rate: u32, frame_samples: usize) -> Self {
        Self {
            sample_rate,
            frame_samples,
            last_report: Instant::now(),
            frames: 0,
            max_frame_duration: Duration::ZERO,
            total_frame_duration: Duration::ZERO,
        }
    }

    fn record_frame(&mut self, duration: Duration) {
        // Track Silero CPU time separately from wall-clock stream latency.
        self.frames += 1;
        self.max_frame_duration = self.max_frame_duration.max(duration);
        self.total_frame_duration += duration;
    }

    fn maybe_report(&mut self) {
        if self.last_report.elapsed() < Duration::from_secs(1) {
            return;
        }

        let avg_frame_ms = if self.frames == 0 {
            0.0
        } else {
            self.total_frame_duration.as_secs_f64() * 1_000.0 / self.frames as f64
        };
        let audio_ms = samples_to_ms(self.frames * self.frame_samples as u64, self.sample_rate);

        debug!(
            frames = self.frames,
            audio_ms,
            avg_frame_ms,
            max_frame_ms = self.max_frame_duration.as_secs_f64() * 1_000.0,
            "VAD metrics"
        );

        self.last_report = Instant::now();
        self.frames = 0;
        self.max_frame_duration = Duration::ZERO;
        self.total_frame_duration = Duration::ZERO;
    }
}

#[cfg(test)]
mod tests {
    use super::VadWorker;

    #[test]
    fn flush_max_chunk_splits_and_keeps_remainder() {
        let mut samples = vec![1.0_f32; 10];
        let (tx, rx) = crossbeam_channel::unbounded();
        let mut index = 0_u64;

        VadWorker::flush_max_chunk(&mut samples, 6, &tx, &mut index);

        assert_eq!(index, 1);
        assert_eq!(samples.len(), 4);
        let chunk = rx.try_recv().unwrap();
        assert_eq!(chunk.samples.len(), 6);
        assert_eq!(chunk.index, 0);
    }
}
