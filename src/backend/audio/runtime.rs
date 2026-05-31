use super::capture::Capture;
use super::device::AudioDevice;
use super::resample::{ResampleConfig, ResampleWorker};
use super::source::AudioSource;
use super::transcript::{StableSegment, TranscriptionWorker, UiChunk};
use super::vad::{SpeechChunk, VadConfig, VadWorker};
use anyhow::{Context, Result};
use ringbuf::{HeapRb, traits::Split};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::broadcast;
use tracing::info;

pub struct AudioRuntime {
    capture: Capture,
    resample: ResampleWorker,
    vad: VadWorker,
    transcription: TranscriptionWorker,

    pub ui_tx: broadcast::Sender<UiChunk>,
    pub segment_tx: broadcast::Sender<StableSegment>,
    pub session_stop_tx: broadcast::Sender<()>,

    stop_flag: Arc<AtomicBool>,
    workers_started: bool,
}

impl AudioRuntime {
    pub fn new(source: AudioSource, moonshine_model_path: &str) -> Result<Self> {
        let audio_device = Self::resolve_audio_device(source)?;
        let config = audio_device.default_input_config()?;
        let sample_rate = config.sample_rate();
        let channels = config.channels() as usize;

        info!(
            source = Self::source_label(source),
            sample_rate, channels, "Initializing unified audio runtime"
        );

        let raw_capacity = sample_rate as usize * channels * 10;
        let transcribe_capacity = 16_000 * 10;

        let (raw_prod, raw_cons) = HeapRb::<f32>::new(raw_capacity).split();
        let (resampled_prod, resampled_cons) = HeapRb::<f32>::new(transcribe_capacity).split();
        let (vad_prod, vad_cons) = HeapRb::<SpeechChunk>::new(100).split();

        let (ui_tx, _) = broadcast::channel::<UiChunk>(100);
        let (segment_tx, _) = broadcast::channel::<StableSegment>(100);
        let (session_stop_tx, _) = broadcast::channel::<()>(10);

        let stop_flag = Arc::new(AtomicBool::new(false));

        let capture = Capture::new(audio_device, raw_prod, stop_flag.clone())?;

        let resample = ResampleWorker::new(
            ResampleConfig {
                input_sample_rate: sample_rate,
                output_sample_rate: 16_000,
                channels,
            },
            raw_cons,
            resampled_prod,
            stop_flag.clone(),
        )?;

        let vad_config = VadConfig::default();
        let vad = VadWorker::new(vad_config, resampled_cons, vad_prod, stop_flag.clone())?;

        let transcription = TranscriptionWorker::new(
            moonshine_model_path,
            vad_cons,
            ui_tx.clone(),
            segment_tx.clone(),
            stop_flag.clone(),
        )?;

        Ok(Self {
            capture,
            resample,
            vad,
            transcription,
            ui_tx,
            segment_tx,
            session_stop_tx,
            stop_flag,
            workers_started: false,
        })
    }

    fn resolve_audio_device(source: AudioSource) -> Result<AudioDevice> {
        AudioDevice::from(source)
    }

    fn source_label(source: AudioSource) -> &'static str {
        match source {
            AudioSource::Monitor => "monitor",
            AudioSource::Mic => "mic",
        }
    }

    pub fn start(&mut self) -> Result<()> {
        info!("Audio runtime starting/resuming...");
        if !self.workers_started {
            self.stop_flag.store(false, Ordering::Relaxed);
            self.transcription.start()?;
            self.vad.start()?;
            self.resample.start()?;
            self.workers_started = true;
        }
        self.capture.start()?;
        Ok(())
    }

    pub fn stop(&mut self) -> Result<()> {
        info!("Audio runtime stopping...");
        self.stop_flag.store(true, Ordering::Relaxed);
        self.capture
            .stop()
            .context("Failed to stop capture worker")?;
        if let Err(e) = self.session_stop_tx.send(()) {
            tracing::error!("Failed to broadcast session stop signal: {:?}", e);
        }

        if self.workers_started {
            self.resample
                .stop()
                .context("Failed to stop resample worker")?;
            self.vad.stop().context("Failed to stop VAD worker")?;
            self.transcription
                .stop()
                .context("Failed to stop transcription worker")?;
            self.workers_started = false;
        }
        Ok(())
    }

    pub fn subscribe_ui(&self) -> broadcast::Receiver<UiChunk> {
        self.ui_tx.subscribe()
    }

    pub fn subscribe_segment(&self) -> broadcast::Receiver<StableSegment> {
        self.segment_tx.subscribe()
    }
}

impl Drop for AudioRuntime {
    fn drop(&mut self) {
        let was_stopped = self.stop_flag.swap(true, Ordering::Relaxed);
        if !was_stopped {
            info!("Audio runtime dropping... stopping workers...");
            if let Err(e) = self.capture.stop() {
                tracing::error!("Error stopping capture in drop: {:?}", e);
            }
            if let Err(e) = self.session_stop_tx.send(()) {
                tracing::error!("Failed to broadcast session stop signal in drop: {:?}", e);
            }
        }
        if self.workers_started {
            if let Err(e) = self.resample.stop() {
                tracing::error!("Error stopping resample worker in drop: {:?}", e);
            }
            if let Err(e) = self.vad.stop() {
                tracing::error!("Error stopping VAD worker in drop: {:?}", e);
            }
            if let Err(e) = self.transcription.stop() {
                tracing::error!("Error stopping transcription worker in drop: {:?}", e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_labels_are_stable() {
        assert_eq!(AudioRuntime::source_label(AudioSource::Monitor), "monitor");
        assert_eq!(AudioRuntime::source_label(AudioSource::Mic), "mic");
    }
}
