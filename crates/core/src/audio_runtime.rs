use anyhow::Result;
use audio::{
    capture::Capture,
    device::AudioDevice,
    resample::{ResampleConfig, ResampleWorker},
    segment::SpeechSegment,
    source::AudioSource,
    vad::{VadConfig, VadWorker},
};
use crossbeam_channel::{Receiver, unbounded};
use ringbuf::{HeapRb, traits::Split};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};
use stt::{
    chunk::TranscriptionChunk, job::TranscriptionJob, path::DEFAULT_WHISPER_MODEL_NAME,
    worker::TranscriptionWorker,
};

/// Owns the live audio-to-transcript pipeline.
///
/// The runtime wires four independent stages together:
///
/// 1. `Capture` reads interleaved device samples from CPAL into a raw ring buffer.
/// 2. `ResampleWorker` drains the raw ring, mixes to mono, resamples to 16 kHz,
///    and pushes samples into the VAD ring.
/// 3. `VadWorker` drains 16 kHz mono samples, detects speech boundaries, and
///    sends finalized `SpeechSegment`s over an unbounded segment channel.
/// 4. `TranscriptionWorker` receives `TranscriptionJob`s and runs Whisper.
pub struct AudioRuntime {
    capture: Capture,
    resample: ResampleWorker,
    vad: VadWorker,
    segment_rx: Option<Receiver<SpeechSegment>>,
    segment_adapter: Option<thread::JoinHandle<()>>,
    transcription_job_tx: Option<crossbeam_channel::Sender<TranscriptionJob>>,
    transcription_worker: TranscriptionWorker,
    transcription_rx: Receiver<TranscriptionChunk>,
    stop: Arc<AtomicBool>,
    stopped: bool,
}

impl AudioRuntime {
    /// Start the default monitor pipeline.
    pub fn start_default() -> Result<Self> {
        Self::new(AudioSource::Monitor)
    }

    /// Start the pipeline for a selected input device.
    pub fn new(source: AudioSource) -> Result<Self> {
        tracing::info!(?source, "creating audio runtime");
        let audio_device = AudioDevice::from(source)?;
        let input_config = audio_device.default_input_config()?;
        let input_sample_rate = input_config.sample_rate();
        let channels = input_config.channels() as usize;

        let stop = Arc::new(AtomicBool::new(false));
        let (segments_tx, segments_rx) = unbounded();
        let (job_tx, job_rx) = unbounded();
        let (transcription_tx, transcription_rx) = unbounded();
        let (raw_producer, raw_consumer) = ring_for(input_sample_rate, channels)?;
        let (vad_producer, vad_consumer) = ring_for(16_000, 1)?;

        tracing::info!(
            input_sample_rate,
            channels,
            raw_ring_seconds = 10,
            vad_ring_seconds = 10,
            "audio runtime buffers configured"
        );

        let capture = Capture::new(audio_device, raw_producer, stop.clone())?;
        let resample = ResampleWorker::new(
            ResampleConfig {
                input_sample_rate,
                output_sample_rate: 16_000,
                channels,
                batch_samples: 1024,
            },
            raw_consumer,
            vad_producer,
            stop.clone(),
        )?;
        let vad = VadWorker::new(
            VadConfig::default(),
            vad_consumer,
            segments_tx,
            stop.clone(),
        )?;
        let transcription_worker = TranscriptionWorker::new(
            DEFAULT_WHISPER_MODEL_NAME,
            job_rx,
            transcription_tx,
            stop.clone(),
        )?;

        Ok(Self {
            capture,
            resample,
            vad,
            segment_rx: Some(segments_rx),
            segment_adapter: None,
            transcription_job_tx: Some(job_tx),
            transcription_worker,
            transcription_rx,
            stop,
            stopped: false,
        })
    }

    /// Start all workers.
    pub fn start(&mut self) -> Result<()> {
        tracing::info!("audio runtime starting");
        self.stop.store(false, Ordering::Relaxed);
        self.transcription_worker.start()?;
        self.segment_adapter = Some(spawn_segment_adapter(
            self.segment_rx
                .take()
                .ok_or_else(|| anyhow::anyhow!("segment receiver already taken"))?,
            self.transcription_job_tx
                .take()
                .ok_or_else(|| anyhow::anyhow!("transcription job sender already taken"))?,
            self.stop.clone(),
        ));
        self.vad.start()?;
        self.resample.start()?;
        self.capture.start()?;
        tracing::info!("audio runtime started");
        Ok(())
    }

    /// Stop all workers.
    pub fn stop(&mut self) {
        if self.stopped {
            return;
        }

        self.stopped = true;
        tracing::info!("audio runtime stopping");
        self.stop.store(true, Ordering::Relaxed);

        let _ = self.capture.stop();
        self.resample.stop();
        self.vad.stop();
        self.transcription_worker.stop();

        if let Some(adapter) = self.segment_adapter.take() {
            let _ = adapter.join();
        }

        tracing::info!("audio runtime stopped");
    }

    /// Clone the transcription receiver for downstream consumers.
    pub fn transcriptions(&self) -> Receiver<TranscriptionChunk> {
        self.transcription_rx.clone()
    }
}

impl Drop for AudioRuntime {
    fn drop(&mut self) {
        self.stop();
    }
}

fn ring_for(
    sample_rate: u32,
    channels: usize,
) -> Result<(ringbuf::HeapProd<f32>, ringbuf::HeapCons<f32>)> {
    anyhow::ensure!(sample_rate > 0, "sample_rate must be > 0");
    anyhow::ensure!(channels > 0, "channels must be > 0");

    let capacity = sample_rate as usize * channels * 10;
    let ring = HeapRb::<f32>::try_new(capacity)?;
    Ok(ring.split())
}

fn spawn_segment_adapter(
    segment_rx: Receiver<SpeechSegment>,
    job_tx: crossbeam_channel::Sender<TranscriptionJob>,
    stop: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let poll = Duration::from_millis(10);
        let mut stats = AdapterStats::new();

        tracing::info!("segment adapter started");

        loop {
            match segment_rx.recv_timeout(poll) {
                Ok(segment) => {
                    if !forward_segment(&job_tx, segment, &mut stats) {
                        break;
                    }

                    while let Ok(segment) = segment_rx.try_recv() {
                        if !forward_segment(&job_tx, segment, &mut stats) {
                            return;
                        }
                    }
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                    if stop.load(Ordering::Relaxed) {
                        while let Ok(segment) = segment_rx.try_recv() {
                            if !forward_segment(&job_tx, segment, &mut stats) {
                                return;
                            }
                        }
                        break;
                    }
                }
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
            }
        }

        tracing::info!("segment adapter stopped");
    })
}

fn forward_segment(
    job_tx: &crossbeam_channel::Sender<TranscriptionJob>,
    segment: SpeechSegment,
    stats: &mut AdapterStats,
) -> bool {
    let started_at = Instant::now();
    let index = segment.index;
    let samples = segment.samples.len();
    let audio_ms = samples as f64 / 16_000.0 * 1_000.0;
    let start_ms = segment.start_ms;
    let end_ms = segment.end_ms;

    let sent = job_tx
        .send(TranscriptionJob::new(
            segment.index,
            segment.samples,
            start_ms,
            end_ms,
        ))
        .is_ok();

    let send_duration = started_at.elapsed();
    if sent {
        tracing::debug!(
            index,
            start_ms,
            end_ms,
            samples,
            audio_ms,
            send_ms = send_duration.as_secs_f64() * 1_000.0,
            "segment forwarded to STT job queue"
        );
        stats.record(send_duration);
    } else {
        tracing::warn!(index, samples, "STT job receiver closed; dropping segment");
    }

    sent
}

struct AdapterStats {
    last_report: Instant,
    forwarded: u64,
    max_send_duration: Duration,
}

impl AdapterStats {
    fn new() -> Self {
        Self {
            last_report: Instant::now(),
            forwarded: 0,
            max_send_duration: Duration::ZERO,
        }
    }

    fn record(&mut self, send_duration: Duration) {
        self.forwarded += 1;
        self.max_send_duration = self.max_send_duration.max(send_duration);

        if self.last_report.elapsed() < Duration::from_secs(1) {
            return;
        }

        tracing::debug!(
            forwarded = self.forwarded,
            max_send_ms = self.max_send_duration.as_secs_f64() * 1_000.0,
            "segment adapter metrics"
        );

        self.last_report = Instant::now();
        self.forwarded = 0;
        self.max_send_duration = Duration::ZERO;
    }
}
