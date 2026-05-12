use anyhow::{Context, Result};
use ringbuf::{HeapCons, HeapProd, traits::Consumer, traits::Producer};
use rubato::{
    Async, FixedAsync, Resampler, SincInterpolationParameters, SincInterpolationType,
    WindowFunction, audioadapter_buffers::direct::SequentialSliceOfVecs, calculate_cutoff,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

/// Resample worker configuration.
#[derive(Debug, Clone)]
pub struct ResampleConfig {
    /// Input sample rate reported by CPAL.
    pub input_sample_rate: u32,
    /// Output sample rate used by VAD/STT. Tayori currently uses 16 kHz.
    pub output_sample_rate: u32,
    /// Number of input channels coming from CPAL.
    pub channels: usize,
    /// Wake-up batch size hint in samples.
    pub batch_samples: usize,
}

impl Default for ResampleConfig {
    fn default() -> Self {
        Self {
            input_sample_rate: 16_000,
            output_sample_rate: 16_000,
            channels: 1,
            batch_samples: 1024,
        }
    }
}

/// Background worker that converts raw capture audio into output sample rate.
///
/// Own input consumer, output producer, and background thread. Pipeline owner
/// creates rings and passes endpoints into worker.
pub struct ResampleWorker {
    config: ResampleConfig,
    input: Option<HeapCons<f32>>,
    output: Option<HeapProd<f32>>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl ResampleWorker {
    /// Create worker from existing input/output ring endpoints.
    pub fn new(
        config: ResampleConfig,
        input: HeapCons<f32>,
        output: HeapProd<f32>,
        stop: Arc<AtomicBool>,
    ) -> Result<Self> {
        anyhow::ensure!(
            config.input_sample_rate > 0,
            "input_sample_rate must be > 0"
        );
        anyhow::ensure!(
            config.output_sample_rate > 0,
            "output_sample_rate must be > 0"
        );
        anyhow::ensure!(config.channels > 0, "channels must be > 0");
        anyhow::ensure!(config.batch_samples > 0, "batch_samples must be > 0");

        Ok(Self {
            config,
            input: Some(input),
            output: Some(output),
            stop,
            worker: None,
        })
    }

    /// Convert interleaved audio into mono by averaging channels per frame.
    ///
    /// Example:
    /// - stereo `L=1.0, R=0.0` becomes mono `0.5`
    /// - mono input passes through unchanged when `channels == 1`
    pub fn to_mono(input: &[f32], channels: usize) -> Result<Vec<f32>> {
        anyhow::ensure!(channels > 0, "channels must be > 0");

        if channels == 1 {
            return Ok(input.to_vec());
        }

        anyhow::ensure!(
            input.len().is_multiple_of(channels),
            "input sample count not divisible by channels"
        );

        let mut mono = Vec::with_capacity(input.len() / channels);

        for frame in input.chunks_exact(channels) {
            let sum: f32 = frame.iter().copied().sum();
            mono.push(sum / channels as f32);
        }

        Ok(mono)
    }

    /// Resample mono audio to target output sample rate.
    ///
    /// Input must already be mono. This function does not mix channels.
    pub fn resample(
        input: &[f32],
        input_sample_rate: u32,
        output_sample_rate: u32,
    ) -> Result<Vec<f32>> {
        if input_sample_rate == output_sample_rate {
            return Ok(input.to_vec());
        }

        let mut resampler = StreamingResampler::new(input_sample_rate, output_sample_rate)?;
        let mut output = Vec::new();
        resampler.push(input, &mut output)?;
        resampler.finish(&mut output)?;

        Ok(output)
    }

    /// Start background worker thread.
    pub fn start(&mut self) -> Result<()> {
        anyhow::ensure!(self.worker.is_none(), "resample worker already started");

        let mut input = self
            .input
            .take()
            .ok_or_else(|| anyhow::anyhow!("input consumer already taken"))?;
        let mut output = self
            .output
            .take()
            .ok_or_else(|| anyhow::anyhow!("output producer already taken"))?;
        let stop = self.stop.clone();
        let config = self.config.clone();

        self.worker = Some(thread::spawn(move || {
            let mut batch = Vec::<f32>::with_capacity(config.batch_samples);
            // Rubato keeps fractional-position state internally, so reuse one
            // streaming resampler for the whole worker instead of rebuilding per batch.
            let mut resampler = match StreamingResampler::new(
                config.input_sample_rate,
                config.output_sample_rate,
            ) {
                Ok(resampler) => resampler,
                Err(err) => {
                    tracing::error!(?err, "failed to initialize audio resampler");
                    return;
                }
            };
            let mut stats = ResampleStats::new(
                config.input_sample_rate,
                config.output_sample_rate,
                config.channels,
            );

            info!(
                input_sample_rate = config.input_sample_rate,
                output_sample_rate = config.output_sample_rate,
                channels = config.channels,
                "resample worker started"
            );

            while !stop.load(Ordering::Relaxed) {
                let loop_started_at = Instant::now();
                batch.clear();

                // Drain everything currently available. Capture keeps writing
                // to the raw ring while this worker converts previous batches.
                while let Some(sample) = input.try_pop() {
                    batch.push(sample);
                }

                if batch.is_empty() {
                    thread::sleep(Duration::from_millis(10));
                    continue;
                }

                // Only complete interleaved frames can be mixed to mono. Any
                // tiny incomplete tail is ignored because CPAL should deliver full frames.
                let usable_len = batch.len() / config.channels * config.channels;
                if usable_len == 0 {
                    continue;
                }

                // Collapse N channels to one mono stream before resampling.
                let mono = match Self::to_mono(&batch[..usable_len], config.channels) {
                    Ok(mono) => mono,
                    Err(err) => {
                        warn!(?err, "failed to mix audio to mono");
                        continue;
                    }
                };

                let mut resampled = Vec::new();
                // `push` may output zero samples until Rubato has enough input.
                if let Err(err) = resampler.push(&mono, &mut resampled) {
                    warn!(?err, "failed to resample audio");
                    continue;
                }

                let output_samples = resampled.len();
                let mut dropped = 0_u64;
                // Push 16 kHz mono samples into the VAD ring. Drops here mean
                // VAD is not keeping up or the ring capacity is too small.
                for sample in resampled {
                    if output.try_push(sample).is_err() {
                        dropped += 1;
                    }
                }

                if dropped > 0 {
                    warn!(dropped, "audio VAD ring full; dropping resampled samples");
                }

                stats.record(
                    usable_len as u64,
                    mono.len() as u64,
                    output_samples as u64,
                    dropped,
                    loop_started_at.elapsed(),
                );
            }

            let mut leftover = Vec::new();
            // Flush Rubato's delayed samples during shutdown so final audio is
            // not stranded in the resampler's internal buffer.
            if let Err(err) = resampler.finish(&mut leftover) {
                warn!(?err, "failed to flush final resampler output");
            } else {
                for sample in leftover {
                    if output.try_push(sample).is_err() {
                        warn!("audio output buffer full; dropping sample");
                    }
                }
            }
        }));

        Ok(())
    }

    /// Stop worker thread.
    pub fn stop(&mut self) {
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

struct StreamingResampler {
    resampler: Async<f32>,
    pending_input: Vec<f32>,
    input_buffer: Vec<Vec<f32>>,
    output_buffer: Vec<Vec<f32>>,
}

impl StreamingResampler {
    fn new(input_sample_rate: u32, output_sample_rate: u32) -> Result<Self> {
        anyhow::ensure!(input_sample_rate > 0, "input_sample_rate must be > 0");
        anyhow::ensure!(output_sample_rate > 0, "output_sample_rate must be > 0");

        let sinc_len = 256;
        let window = WindowFunction::BlackmanHarris2;
        let params = SincInterpolationParameters {
            sinc_len,
            f_cutoff: calculate_cutoff(sinc_len, window),
            interpolation: SincInterpolationType::Cubic,
            oversampling_factor: 256,
            window,
        };

        let ratio = output_sample_rate as f64 / input_sample_rate as f64;
        // FixedAsync::Input means Rubato tells us how many input frames it wants
        // next. We buffer until that many frames are available.
        let resampler = Async::<f32>::new_sinc(ratio, 1.05, &params, 1024, 1, FixedAsync::Input)
            .context("failed to create Rubato resampler")?;

        let input_max = resampler.input_frames_next();
        let output_max = resampler.output_frames_max();

        Ok(Self {
            resampler,
            pending_input: Vec::with_capacity(input_max * 2),
            input_buffer: vec![Vec::with_capacity(input_max)],
            output_buffer: vec![Vec::with_capacity(output_max)],
        })
    }

    fn push(&mut self, input: &[f32], output: &mut Vec<f32>) -> Result<()> {
        if input.is_empty() {
            return Ok(());
        }

        // Append new mono input to any previous tail that was too short for
        // Rubato's next input window.
        self.pending_input.extend_from_slice(input);
        self.flush_ready(output)
    }

    fn finish(&mut self, output: &mut Vec<f32>) -> Result<()> {
        if self.pending_input.is_empty() {
            return Ok(());
        }

        let needed_input = self.resampler.input_frames_next();
        if self.pending_input.len() < needed_input {
            // Pad only on final flush. Padding during normal streaming would add
            // artificial silence and distort timing.
            self.pending_input.resize(needed_input, 0.0);
        }

        self.flush_ready(output)
    }

    fn flush_ready(&mut self, output: &mut Vec<f32>) -> Result<()> {
        loop {
            let needed_input = self.resampler.input_frames_next();

            if self.pending_input.len() < needed_input {
                break;
            }

            // Feed exactly the number of input frames Rubato requested.
            let chunk: Vec<f32> = self.pending_input.drain(..needed_input).collect();
            self.input_buffer[0].clear();
            self.input_buffer[0].extend_from_slice(&chunk);

            let expected_output = self.resampler.output_frames_next();
            self.output_buffer[0].resize(expected_output, 0.0);

            // Rubato operates on channel buffers. We have one mono channel, so
            // both adapters wrap a single Vec.
            let input_adapter = SequentialSliceOfVecs::new(&self.input_buffer, 1, needed_input)
                .context("failed to create Rubato input adapter")?;
            let mut output_adapter =
                SequentialSliceOfVecs::new_mut(&mut self.output_buffer, 1, expected_output)
                    .context("failed to create Rubato output adapter")?;

            let (_frames_in, frames_out) = self
                .resampler
                .process_into_buffer(&input_adapter, &mut output_adapter, None)
                .context("Rubato resampling failed")?;

            output.extend_from_slice(&self.output_buffer[0][..frames_out]);
        }

        Ok(())
    }
}

struct ResampleStats {
    input_sample_rate: u32,
    output_sample_rate: u32,
    channels: usize,
    last_report: Instant,
    input_samples: u64,
    mono_samples: u64,
    output_samples: u64,
    dropped_output_samples: u64,
    batches: u64,
    max_batch_duration: Duration,
}

impl ResampleStats {
    fn new(input_sample_rate: u32, output_sample_rate: u32, channels: usize) -> Self {
        Self {
            input_sample_rate,
            output_sample_rate,
            channels,
            last_report: Instant::now(),
            input_samples: 0,
            mono_samples: 0,
            output_samples: 0,
            dropped_output_samples: 0,
            batches: 0,
            max_batch_duration: Duration::ZERO,
        }
    }

    fn record(
        &mut self,
        input_samples: u64,
        mono_samples: u64,
        output_samples: u64,
        dropped_output_samples: u64,
        batch_duration: Duration,
    ) {
        // Track throughput and worst batch time so latency spikes can be spotted
        // without logging every batch.
        self.input_samples += input_samples;
        self.mono_samples += mono_samples;
        self.output_samples += output_samples;
        self.dropped_output_samples += dropped_output_samples;
        self.batches += 1;
        self.max_batch_duration = self.max_batch_duration.max(batch_duration);

        if self.last_report.elapsed() < Duration::from_secs(1) {
            return;
        }

        let input_audio_ms = if self.input_sample_rate == 0 || self.channels == 0 {
            0.0
        } else {
            self.input_samples as f64 / self.channels as f64 / self.input_sample_rate as f64
                * 1_000.0
        };
        let output_audio_ms = if self.output_sample_rate == 0 {
            0.0
        } else {
            self.output_samples as f64 / self.output_sample_rate as f64 * 1_000.0
        };

        debug!(
            batches = self.batches,
            input_samples = self.input_samples,
            mono_samples = self.mono_samples,
            output_samples = self.output_samples,
            dropped_output_samples = self.dropped_output_samples,
            input_audio_ms,
            output_audio_ms,
            max_batch_ms = self.max_batch_duration.as_secs_f64() * 1_000.0,
            "resample metrics"
        );

        self.last_report = Instant::now();
        self.input_samples = 0;
        self.mono_samples = 0;
        self.output_samples = 0;
        self.dropped_output_samples = 0;
        self.batches = 0;
        self.max_batch_duration = Duration::ZERO;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixes_stereo_to_mono_by_average() {
        let mono = ResampleWorker::to_mono(&[1.0, 0.0, 0.5, 0.5], 2).unwrap();
        assert_eq!(mono, vec![0.5, 0.5]);
    }

    #[test]
    fn passes_through_mono() {
        let mono = ResampleWorker::to_mono(&[0.1, 0.2, 0.3], 1).unwrap();
        assert_eq!(mono, vec![0.1, 0.2, 0.3]);
    }

    #[test]
    fn resample_keeps_mono_input_unchanged_when_rates_match() {
        let input = vec![0.0_f32, 0.25, 0.5, 0.75, 1.0];
        let output = ResampleWorker::resample(&input, 16_000, 16_000).unwrap();
        assert_eq!(output, input);
    }

    #[test]
    fn resample_downsample_stays_near_expected_length() {
        let input = vec![0.5_f32; 48_000];
        let output = ResampleWorker::resample(&input, 48_000, 16_000).unwrap();

        assert!(!output.is_empty());
        assert!(output.len() > 15_000);
        assert!(output.len() < 17_000);
    }
}
