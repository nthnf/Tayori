use anyhow::{Context, Result};
use ringbuf::HeapCons;
use ringbuf::traits::Consumer;
use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;
use tokio::sync::broadcast;
use tracing::{error, info};

use crate::backend::models::moonshine::{Quantization, StreamingModel};

use super::vad::SpeechChunk;

#[derive(Clone, Debug)]
pub struct UiChunk {
    pub id: usize,
    pub draft_text: String,
    pub newly_stable_text: Option<String>,
}

#[derive(Clone, Debug)]
pub struct StableSegment {
    pub id: uuid::Uuid,
    pub start_time: u64,
    pub end_time: u64,
    pub full_text: String,
    pub is_end_of_speech: bool,
}

pub struct TranscriptionWorker {
    input: Option<HeapCons<SpeechChunk>>,
    ui_tx: broadcast::Sender<UiChunk>,
    segment_tx: broadcast::Sender<StableSegment>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<Result<()>>>,
    moonshine_model_path: String,
}

impl TranscriptionWorker {
    pub fn new(
        moonshine_model_path: &str,
        input: HeapCons<SpeechChunk>,
        ui_tx: broadcast::Sender<UiChunk>,
        segment_tx: broadcast::Sender<StableSegment>,
        stop: Arc<AtomicBool>,
    ) -> Result<Self> {
        Ok(Self {
            input: Some(input),
            ui_tx,
            segment_tx,
            stop,
            worker: None,
            moonshine_model_path: moonshine_model_path.to_string(),
        })
    }

    pub fn start(&mut self) -> Result<()> {
        anyhow::ensure!(
            self.worker.is_none(),
            "transcription worker already started"
        );

        let mut input = self
            .input
            .take()
            .ok_or_else(|| anyhow::anyhow!("transcription input already taken"))?;
        let ui_tx = self.ui_tx.clone();
        let segment_tx = self.segment_tx.clone();
        let stop = self.stop.clone();
        let moonshine_model_path = self.moonshine_model_path.clone();

        // Load the model synchronously on the caller's thread so errors bubble up immediately
        let mut stt = StreamingModel::load(
            &PathBuf::from(&moonshine_model_path),
            1, // Limit to 1 thread per session to avoid CPU starvation
            &Quantization::default(),
        )
        .context("Failed to load Moonshine STT model")?;

        self.worker = Some(thread::spawn(move || -> Result<()> {
            let result = panic::catch_unwind(AssertUnwindSafe(move || -> Result<()> {
                info!("transcription worker started (streaming moonshine)");
                tracing::debug!("Moonshine loaded! Entering processing loop.");
                drain_and_process(&mut input, &mut stt, ui_tx, segment_tx, stop);
                Ok(())
            }));

            match result {
                Ok(res) => res,
                Err(err) => Err(anyhow::anyhow!("transcription worker panicked: {:?}", err)),
            }
        }));

        Ok(())
    }

    pub fn stop(&mut self) -> Result<()> {
        if let Some(handle) = self.worker.take() {
            handle.join().map_err(|e| {
                anyhow::anyhow!("transcription worker thread panicked on join: {:?}", e)
            })??;
        }
        Ok(())
    }
}

fn drain_and_process(
    input: &mut HeapCons<SpeechChunk>,
    stt: &mut StreamingModel,
    ui_tx: broadcast::Sender<UiChunk>,
    segment_tx: broadcast::Sender<StableSegment>,
    stop: Arc<AtomicBool>,
) {
    let poll_interval = Duration::from_millis(10);

    let mut current_id = 0;
    let mut segment_start_ms = 0;

    let mut state = stt.create_state();
    let mut chunks_since_last_decode = 0;
    let mut total_chunks_since_reset = 0;
    const MAX_CONTINUOUS_CHUNKS: usize = 15;

    loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }

        if let Some(chunk) = input.try_pop() {
            if chunk.samples.is_empty() {
                continue;
            }

            if state.frame_count == 0 {
                segment_start_ms = chunk.start_ms;
            }

            if let Err(e) = stt.process_audio_chunk(&mut state, &chunk.samples) {
                error!("STT inference failed: {:?}", e);
                continue;
            }

            chunks_since_last_decode += 1;
            total_chunks_since_reset += 1;

            let force_flush = total_chunks_since_reset >= MAX_CONTINUOUS_CHUNKS;
            let is_end = chunk.is_end_of_speech || force_flush;

            // To provide real-time updates without mutating the persistent streaming state,
            // we clone the state. This is inexpensive enough for 640ms increments.
            // We only decode if it's the end of speech, OR every 4 chunks (~2.5s) to save CPU
            // while still appearing real-time.
            if is_end || chunks_since_last_decode >= 3 {
                chunks_since_last_decode = 0;
                let mut draft_state = state.clone();

                if let Ok(text) = stt.decode_current_state(&mut draft_state, is_end) {
                    let current_text = text.trim().to_string();
                    if !is_end {
                        tracing::info!("STT Draft: {}", current_text);
                        if let Err(e) = ui_tx.send(UiChunk {
                            id: current_id,
                            draft_text: current_text.clone(),
                            newly_stable_text: None,
                        }) {
                            tracing::error!("Failed to broadcast STT draft: {:?}", e);
                        }
                    } else {
                        // Send empty draft to clear UI explicitly before final chunk
                        if let Err(e) = ui_tx.send(UiChunk {
                            id: current_id,
                            draft_text: String::new(),
                            newly_stable_text: None,
                        }) {
                            tracing::error!("Failed to broadcast empty STT draft: {:?}", e);
                        }
                    }

                    if is_end {
                        if !current_text.is_empty() {
                            if let Err(e) = segment_tx.send(StableSegment {
                                id: uuid::Uuid::new_v4(),
                                start_time: segment_start_ms,
                                end_time: chunk.end_ms,
                                full_text: current_text,
                                is_end_of_speech: chunk.is_end_of_speech,
                            }) {
                                tracing::error!("Failed to broadcast stable segment: {:?}", e);
                            }
                            current_id += 1;
                        }

                        stt.reset_state(&mut state);
                        total_chunks_since_reset = 0;

                        if force_flush && !chunk.is_end_of_speech {
                            // Re-feed the exact same chunk into the new state so it acts as the starting overlap context
                            if let Err(e) = stt.process_audio_chunk(&mut state, &chunk.samples) {
                                error!("STT inference failed on overlap chunk: {:?}", e);
                            } else {
                                total_chunks_since_reset = 1;
                                segment_start_ms = chunk.start_ms;
                            }
                        }
                    }
                } else {
                    error!("STT decode failed");
                }
            }
        } else {
            thread::sleep(poll_interval);
        }
    }
}
