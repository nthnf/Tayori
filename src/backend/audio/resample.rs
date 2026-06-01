use anyhow::{Context, Result};
use ringbuf::{HeapCons, HeapProd, traits::Consumer, traits::Observer, traits::Producer};
use rubato::{
    Async, FixedAsync, Resampler, SincInterpolationParameters, SincInterpolationType,
    WindowFunction, audioadapter_buffers::direct::SequentialSliceOfVecs, calculate_cutoff,
};
use std::panic::{self, AssertUnwindSafe};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;
use tracing::{info, warn};

/// Resample worker configuration.
#[derive(Debug, Clone)]
pub struct ResampleConfig {
    /// Input sample rate reported by CPAL.
    pub input_sample_rate: u32,
    /// Output sample rate used by VAD/STT. Tayori currently uses 16 kHz.
    pub output_sample_rate: u32,
    /// Number of input channels coming from CPAL.
    pub channels: usize,
}

/// Background worker that converts raw audio into the target output sample rate.
///
/// This worker runs on its own dedicated OS thread. It performs the following audio
/// processing pipeline:
/// 1. Consumes raw interleaved audio (e.g. stereo 48kHz) from an input ring buffer.
/// 2. Downmixes the interleaved multi-channel audio into a single mono stream.
/// 3. Uses the highly-optimized `rubato` crate to perform Sinc interpolation
///    to convert the sample rate (e.g. 48kHz -> 16kHz).
/// 4. Pushes the finalized mono 16kHz audio into an output ring buffer.
///
/// The worker utilizes `FixedAsync::Input`, meaning it buffers exactly enough raw audio
/// frames to satisfy the next chunk of the mathematical sinc filter before processing.
pub struct ResampleWorker {
    config: ResampleConfig,
    input: Option<HeapCons<f32>>,
    output: Option<HeapProd<f32>>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<Result<()>>>,
}

impl ResampleWorker {
    /// Initializes the worker and validates the audio configuration.
    ///
    /// This method merely prepares the worker and takes ownership of the ring buffer
    /// endpoints. The background processing thread will not begin until `start()` is called.
    ///
    /// # Arguments
    ///
    /// * `config` - The input and output sample rate and channel configuration.
    /// * `input` - The consumer end of the raw audio ring buffer (from CPAL).
    /// * `output` - The producer end of the downsampled, mono ring buffer (to VAD/STT).
    /// * `stop` - A shared atomic flag to gracefully shut down the worker.
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

        Ok(Self {
            config,
            input: Some(input),
            output: Some(output),
            stop,
            worker: None,
        })
    }

    /// Spawns the dedicated background thread and begins the continuous resampling loop.
    ///
    /// # Thread Lifecycle
    /// 1. **Setup**: Calculates the exact Sinc interpolation filter parameters based on the ratio
    ///    between input and output sample rates.
    /// 2. **Loop**:
    ///    - Polls the input ring buffer. If empty, sleeps for a few milliseconds.
    ///    - Drains raw samples, averages the channels down to mono, and accumulates them.
    ///    - When enough mono samples are accumulated to satisfy Rubato's `input_frames_next()`,
    ///      it executes the filter.
    ///    - The resulting resampled audio is immediately pushed to the transcription worker's ring buffer.
    /// 3. **Shutdown**: When the `stop` flag is toggled, it breaks the loop, pads the remaining
    ///    audio with silence to flush the final filter state, and exits.
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

        self.worker = Some(thread::spawn(move || -> Result<()> {
            let mut run = || -> Result<()> {
                // 1. Setup Rubato
                let ratio = config.output_sample_rate as f64 / config.input_sample_rate as f64;
                let sinc_len = 256;
                let window = WindowFunction::BlackmanHarris2;
                let params = SincInterpolationParameters {
                    sinc_len,
                    f_cutoff: calculate_cutoff(sinc_len, window),
                    interpolation: SincInterpolationType::Cubic,
                    oversampling_factor: 256,
                    window,
                };

                // FixedAsync::Input means Rubato tells us how many input frames it wants
                // next. We buffer until that many frames are available.
                let mut resampler =
                    Async::<f32>::new_sinc(ratio, 1.05, &params, 1024, 1, FixedAsync::Input)
                        .context("failed to create Rubato resampler")?;

                // 2. Pre-allocate core buffers
                let mut pending_input = Vec::with_capacity(resampler.input_frames_max() * 2);
                let mut input_buffer = vec![Vec::with_capacity(resampler.input_frames_max())];
                let mut output_buffer = vec![Vec::with_capacity(resampler.output_frames_max())];

                info!(
                    input_sample_rate = config.input_sample_rate,
                    output_sample_rate = config.output_sample_rate,
                    channels = config.channels,
                    "resample worker started"
                );

                // 3. The tight loop
                loop {
                    // Step 1: Drain interleaved audio from the hardware capture ring buffer.
                    // We only process full interleaved frames (e.g. 2 samples for stereo)
                    // to avoid channel misalignment.
                    while input.occupied_len() >= config.channels {
                        let mut sum = 0.0;
                        for _ in 0..config.channels {
                            if let Some(sample) = input.try_pop() {
                                sum += sample;
                            }
                        }
                        // Step 2: Mixdown to Mono.
                        // We average the channels together and push into our pending accumulation buffer.
                        pending_input.push(sum / config.channels as f32);
                    }

                    // Step 3: Process the accumulated mono frames with Rubato.
                    // We loop to catch up if we have many frames stored.
                    loop {
                        // Step 3a: Ask Rubato how many mono samples it needs to compute the next chunk.
                        let needed = resampler.input_frames_next();

                        // If we haven't accumulated enough mono samples from CPAL yet, break and wait.
                        if pending_input.len() < needed {
                            break;
                        }

                        // Step 3b: Move the exact amount of frames needed into the input adapter buffer.
                        input_buffer[0].clear();
                        input_buffer[0].extend(pending_input.drain(..needed));

                        // Prepare the output buffer with the exact size Rubato expects to produce.
                        let expected_out = resampler.output_frames_next();
                        output_buffer[0].resize(expected_out, 0.0);

                        let input_adapter = SequentialSliceOfVecs::new(&input_buffer, 1, needed)
                            .context("failed to create Rubato input adapter")?;
                        let mut output_adapter =
                            SequentialSliceOfVecs::new_mut(&mut output_buffer, 1, expected_out)
                                .context("failed to create Rubato output adapter")?;

                        // Step 4: Resample!
                        // The input_adapter passes the chunks into the Sinc filter, and the math
                        // is processed into the output_adapter.
                        let (_frames_in, frames_out) = resampler
                            .process_into_buffer(&input_adapter, &mut output_adapter, None)
                            .context("Rubato resampling failed")?;

                        // Step 5: Push output straight into the next Ringbuffer (to the Transcription Worker).
                        let mut dropped = 0;
                        for &sample in &output_buffer[0][..frames_out] {
                            if output.try_push(sample).is_err() {
                                dropped += 1;
                            }
                        }
                        if dropped > 0 {
                            warn!(dropped, "audio VAD ring full; dropping resampled samples");
                        }
                    }

                    // Step 6: Check for graceful shutdown between chunks.
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }

                    // Step 7: Sleep to prevent a 100% CPU busy-wait loop if the hardware buffer is empty.
                    thread::sleep(Duration::from_millis(10));
                }

                // Step 8: Shutdown / Flush
                // If there are leftover samples that didn't make a full chunk, we pad them with zeros
                // to force the final interpolation frames out of Rubato's internal state.
                if !pending_input.is_empty() {
                    let needed = resampler.input_frames_next();
                    // Pad only on final flush to force out delayed samples.
                    pending_input.resize(needed, 0.0);

                    input_buffer[0].clear();
                    input_buffer[0].append(&mut pending_input);

                    let expected_out = resampler.output_frames_next();
                    output_buffer[0].resize(expected_out, 0.0);

                    let input_adapter = SequentialSliceOfVecs::new(&input_buffer, 1, needed)
                        .context("failed to create Rubato flush input adapter")?;
                    let mut output_adapter =
                        SequentialSliceOfVecs::new_mut(&mut output_buffer, 1, expected_out)
                            .context("failed to create Rubato flush output adapter")?;

                    if let Ok((_, frames_out)) =
                        resampler.process_into_buffer(&input_adapter, &mut output_adapter, None)
                    {
                        for &sample in &output_buffer[0][..frames_out] {
                            if let Err(_e) = output.try_push(sample) {
                                tracing::trace!(
                                    "Output ringbuffer full during flush, dropped sample"
                                );
                            }
                        }
                    }
                }

                Ok(())
            };

            let result = panic::catch_unwind(AssertUnwindSafe(&mut run));
            match result {
                Ok(res) => res,
                Err(err) => Err(anyhow::anyhow!("resample worker panicked: {:?}", err)),
            }
        }));

        Ok(())
    }

    /// Stop worker thread.
    pub fn stop(&mut self) -> Result<()> {
        if let Some(worker) = self.worker.take() {
            worker.join().map_err(|e| {
                anyhow::anyhow!("resample worker thread panicked on join: {:?}", e)
            })??;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ringbuf::HeapRb;
    use ringbuf::traits::Split;

    #[test]
    fn resample_worker_lifecycle() {
        let config = ResampleConfig {
            input_sample_rate: 48_000,
            output_sample_rate: 16_000,
            channels: 2,
        };

        let (raw_prod, raw_cons) = HeapRb::<f32>::new(1024).split();
        let (vad_prod, mut vad_cons) = HeapRb::<f32>::new(1024).split();
        let stop = Arc::new(AtomicBool::new(false));

        let mut worker = ResampleWorker::new(config, raw_cons, vad_prod, stop.clone()).unwrap();
        worker.start().unwrap();

        // Push 1000 frames of stereo audio
        // [1.0, 0.0] -> mono 0.5
        let mut input_prod = raw_prod;
        for _ in 0..1000 {
            let _ = input_prod.try_push(1.0);
            let _ = input_prod.try_push(0.0);
        }

        // Wait a bit for the worker to process
        thread::sleep(Duration::from_millis(100));

        stop.store(true, Ordering::Relaxed);
        worker.stop().unwrap();

        // Check if we got any output.
        let output_samples: Vec<f32> = vad_cons.pop_iter().collect();
        assert!(
            !output_samples.is_empty(),
            "should have produced resampled samples"
        );

        // Skip initial ramp-up samples and check the middle of the stream
        let mid = output_samples.len() / 2;
        if let Some(&sample) = output_samples.get(mid) {
            assert!(
                sample > 0.4 && sample < 0.6,
                "output sample at index {} ({}) should be near 0.5",
                mid,
                sample
            );
        }
    }
}
