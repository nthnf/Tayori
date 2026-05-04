use std::{
    thread,
    time::{Duration, Instant},
};

use anyhow::Result;
use audio::{
    CaptureDevice, CpalCapture, CpalCaptureConfig, SileroVadConfig, SileroVadSegmenter,
    SpeechSegment,
};
use crossbeam_channel::{Receiver, Sender, TrySendError, bounded};
use stt::{SttConfig, Transcription, WhisperEngine, whisper_model_path};

fn main() -> Result<()> {
    let capture = CpalCapture::new(CpalCaptureConfig {
        device: CaptureDevice::DefaultMonitor,
        target_sample_rate: 16_000,
        ring_seconds: 10,
        output_frame_ms: 32,
        frame_channel_capacity: 512,
    });

    let handle = capture.start()?;

    let mut vad = SileroVadSegmenter::new(SileroVadConfig {
        threshold: 0.2,
        min_segment_ms: 300,
        max_segment_ms: 3_000,
        pre_roll_frames: 2,
        ..Default::default()
    })?;

    let (segment_tx, segment_rx) = bounded::<SpeechSegment>(2);
    let (transcript_tx, transcript_rx) = bounded::<Transcription>(16);

    let stt_thread = spawn_stt_worker(segment_rx, transcript_tx);

    let started_at = Instant::now();
    let duration = Duration::from_secs(120);

    while started_at.elapsed() < duration {
        if let Ok(frame) = handle.frames.recv_timeout(Duration::from_millis(100)) {
            if let Some(segment) = vad.push_frame(frame)? {
                send_latest_segment(&segment_tx, segment);
            }
        }

        while let Ok(transcription) = transcript_rx.try_recv() {
            if !transcription.text.is_empty() {
                println!("{}", transcription.text);
            }
        }
    }

    if let Some(segment) = vad.flush() {
        send_latest_segment(&segment_tx, segment);
    }

    drop(segment_tx);

    while let Ok(transcription) = transcript_rx.recv_timeout(Duration::from_millis(500)) {
        if !transcription.text.is_empty() {
            println!("{}", transcription.text);
        }
    }

    handle.stop()?;

    let _ = stt_thread.join();

    Ok(())
}

fn spawn_stt_worker(
    segment_rx: Receiver<SpeechSegment>,
    transcript_tx: Sender<Transcription>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let stt = match WhisperEngine::new(SttConfig {
            single_segment: true,
            ..Default::default()
        }) {
            Ok(stt) => stt,
            Err(err) => {
                eprintln!("failed to initialize STT: {err:?}");
                return;
            }
        };

        while let Ok(segment) = segment_rx.recv() {
            match stt.transcribe_samples(&segment.samples) {
                Ok(transcription) => {
                    let _ = transcript_tx.try_send(transcription);
                }
                Err(err) => {
                    eprintln!("STT failed: {err:?}");
                }
            }
        }
    })
}

fn send_latest_segment(tx: &Sender<SpeechSegment>, segment: SpeechSegment) {
    match tx.try_send(segment) {
        Ok(()) => {}

        Err(TrySendError::Full(segment)) => {
            // Live mode rule:
            // If STT is behind, drop one stale segment and keep the newest one.
            let _ = tx.try_send(segment);
        }

        Err(TrySendError::Disconnected(_)) => {}
    }
}
