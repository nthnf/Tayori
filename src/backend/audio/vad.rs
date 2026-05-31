use crate::backend::models::install::default_silero_path;
use crate::backend::models::silero::{SileroVad, VadEvent};
use anyhow::{Context, Result};
use ringbuf::traits::{Consumer, Producer};
use ringbuf::{HeapCons, HeapProd};
use std::panic::{self, AssertUnwindSafe};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;
use tracing::{info, warn};

/// A chunk of speech emitted by the VAD worker.
#[derive(Debug, Clone)]
pub struct SpeechChunk {
    pub samples: Vec<f32>,
    pub start_ms: u64,
    pub end_ms: u64,
    pub is_end_of_speech: bool,
}

#[derive(Debug, Clone)]
pub struct VadConfig {
    pub threshold: f32,
    pub sample_rate: u32,
    pub frame_samples: usize,
    pub max_chunk_ms: u32,
    pub speech_pad_ms: u32,
    pub min_silence_ms: u32,
}

impl Default for VadConfig {
    fn default() -> Self {
        Self {
            threshold: 0.5,
            sample_rate: 16_000,
            frame_samples: 512, // Silero V5 strictly requires 512 for 16kHz
            max_chunk_ms: 640,  // The "window buffer"
            speech_pad_ms: 100,
            min_silence_ms: 160,
        }
    }
}

pub struct VadWorker {
    input: Option<HeapCons<f32>>,
    output: Option<HeapProd<SpeechChunk>>,
    iterator: Option<SileroVad>,
    stop: Arc<AtomicBool>,
    config: VadConfig,
    worker: Option<thread::JoinHandle<Result<()>>>,
}

impl VadWorker {
    pub fn new(
        config: VadConfig,
        input: HeapCons<f32>,
        output: HeapProd<SpeechChunk>,
        stop: Arc<AtomicBool>,
    ) -> Result<Self> {
        let model_path = default_silero_path(None);
        let iterator = SileroVad::new(
            &model_path,
            config.threshold,
            config.sample_rate,
            config.min_silence_ms,
            config.speech_pad_ms,
        )
        .context("Failed to create silero VAD iterator")?;

        Ok(Self {
            input: Some(input),
            output: Some(output),
            iterator: Some(iterator),
            stop,
            config,
            worker: None,
        })
    }

    pub fn start(&mut self) -> Result<()> {
        anyhow::ensure!(self.worker.is_none(), "VAD worker already started");

        let mut input = self.input.take().context("VAD input already taken")?;
        let mut output = self.output.take().context("VAD output already taken")?;
        let mut iterator = self.iterator.take().context("VAD iterator already taken")?;
        let stop = self.stop.clone();
        let config = self.config.clone();

        self.worker = Some(thread::spawn(move || -> Result<()> {
            let mut run = move || -> Result<()> {
                let mut batch = Vec::<f32>::with_capacity(config.frame_samples);
                let mut speech_samples = Vec::<f32>::new();
                let mut pending_samples = Vec::<f32>::new();

                let mut in_speech = false;
                let mut _speech_started_at_sample = 0_u64;
                let mut chunk_started_at_sample = 0_u64;
                let mut processed_samples = 0_u64;

                let max_chunk_samples =
                    ((config.sample_rate as usize * config.max_chunk_ms as usize) / 1_000).max(1);

                info!("VAD worker started (v5)");

                loop {
                    batch.clear();
                    while let Some(sample) = input.try_pop() {
                        batch.push(sample);
                    }

                    if batch.is_empty() {
                        if stop.load(Ordering::Relaxed) {
                            break;
                        }
                        thread::sleep(Duration::from_millis(5));
                        continue;
                    }

                    pending_samples.extend_from_slice(&batch);
                    let usable_len =
                        pending_samples.len() / config.frame_samples * config.frame_samples;
                    if usable_len == 0 {
                        continue;
                    }

                    for frame in pending_samples[..usable_len].chunks(config.frame_samples) {
                        let event = match iterator.process_chunk(frame) {
                            Ok(event) => event,
                            Err(e) => {
                                warn!("VAD failed to process frame: {:?}", e);
                                continue;
                            }
                        };

                        processed_samples += frame.len() as u64;

                        match event {
                            Some(VadEvent::Start(_ts)) => {
                                in_speech = true;
                                _speech_started_at_sample =
                                    processed_samples.saturating_sub(frame.len() as u64);
                                chunk_started_at_sample = _speech_started_at_sample;
                                speech_samples.clear();
                                speech_samples.extend_from_slice(frame);
                                Self::flush_max_chunk(
                                    &mut speech_samples,
                                    &mut chunk_started_at_sample,
                                    max_chunk_samples,
                                    &mut output,
                                    config.sample_rate,
                                    false,
                                );
                            }
                            Some(VadEvent::End(_ts)) => {
                                if in_speech {
                                    speech_samples.extend_from_slice(frame);
                                    Self::flush_max_chunk(
                                        &mut speech_samples,
                                        &mut chunk_started_at_sample,
                                        max_chunk_samples,
                                        &mut output,
                                        config.sample_rate,
                                        false,
                                    );
                                    Self::flush_chunk(
                                        std::mem::take(&mut speech_samples),
                                        chunk_started_at_sample,
                                        &mut output,
                                        config.sample_rate,
                                        true,
                                    );
                                }
                                in_speech = false;
                            }
                            None => {
                                if in_speech {
                                    speech_samples.extend_from_slice(frame);
                                    Self::flush_max_chunk(
                                        &mut speech_samples,
                                        &mut chunk_started_at_sample,
                                        max_chunk_samples,
                                        &mut output,
                                        config.sample_rate,
                                        false,
                                    );
                                }
                            }
                        }
                    }

                    pending_samples.drain(..usable_len);

                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                }

                if in_speech {
                    speech_samples.extend_from_slice(&pending_samples);
                    Self::flush_chunk(
                        speech_samples,
                        chunk_started_at_sample,
                        &mut output,
                        config.sample_rate,
                        true,
                    );
                }
                Ok(())
            };

            let result = panic::catch_unwind(AssertUnwindSafe(&mut run));
            match result {
                Ok(res) => res,
                Err(err) => Err(anyhow::anyhow!("VAD worker panicked: {:?}", err)),
            }
        }));

        Ok(())
    }

    pub fn stop(&mut self) -> Result<()> {
        if let Some(worker) = self.worker.take() {
            worker
                .join()
                .map_err(|e| anyhow::anyhow!("VAD worker thread panicked on join: {:?}", e))??;
        }
        Ok(())
    }

    fn flush_chunk(
        samples: Vec<f32>,
        start_sample: u64,
        output: &mut HeapProd<SpeechChunk>,
        sample_rate: u32,
        is_end_of_speech: bool,
    ) {
        if samples.is_empty() {
            return;
        }

        let start_ms = samples_to_ms_u64(start_sample, sample_rate);
        let end_ms = samples_to_ms_u64(start_sample + samples.len() as u64, sample_rate);

        let chunk = SpeechChunk {
            samples,
            start_ms,
            end_ms,
            is_end_of_speech,
        };

        while output.try_push(chunk.clone()).is_err() {
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn flush_max_chunk(
        speech_samples: &mut Vec<f32>,
        chunk_started_at_sample: &mut u64,
        max_chunk_samples: usize,
        output: &mut HeapProd<SpeechChunk>,
        sample_rate: u32,
        is_end_of_speech: bool,
    ) {
        while speech_samples.len() >= max_chunk_samples {
            let emitted_samples = max_chunk_samples as u64;
            let mut chunk = Vec::new();
            std::mem::swap(&mut chunk, speech_samples);
            let overflow = if chunk.len() > max_chunk_samples {
                chunk.split_off(max_chunk_samples)
            } else {
                Vec::new()
            };

            Self::flush_chunk(
                chunk,
                *chunk_started_at_sample,
                output,
                sample_rate,
                is_end_of_speech,
            );
            *chunk_started_at_sample += emitted_samples;
            *speech_samples = overflow;
        }
    }
}

fn samples_to_ms_u64(samples: u64, sample_rate: u32) -> u64 {
    if sample_rate == 0 {
        return 0;
    }
    samples.saturating_mul(1_000) / sample_rate as u64
}
