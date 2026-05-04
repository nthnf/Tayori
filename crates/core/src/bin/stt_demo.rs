use std::{
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use anyhow::Result;
use audio::{
    AudioSnapshotRequest, CaptureDevice, CpalCapture, CpalCaptureConfig, FinalReason,
    LiveSnapshotScheduler, LiveSnapshotSchedulerConfig, RollingAudioBuffer, SileroVadConfig,
    SileroVadWatcher, SnapshotKind,
};
use crossbeam_channel::{Receiver, Sender, bounded};
use stt::{
    SttConfig, SttJob, SttJobInbox, SttJobKind, Transcription, WhisperEngine, whisper_model_path,
};

fn main() -> Result<()> {
    let capture = CpalCapture::new(CpalCaptureConfig {
        device: CaptureDevice::DefaultMonitor,
        target_sample_rate: 16_000,
        ring_seconds: 10,
        output_frame_ms: 32,
        frame_channel_capacity: 512,
    });

    let handle = capture.start()?;

    let mut rolling_buffer = RollingAudioBuffer::with_seconds(60, 16_000);

    let mut vad = SileroVadWatcher::new(SileroVadConfig {
        threshold: 0.55,
        frame_samples: 512,
        ..Default::default()
    })?;

    let mut snapshot_scheduler = LiveSnapshotScheduler::new(LiveSnapshotSchedulerConfig {
        min_transcribe_samples: 16_000 / 2,
        partial_interval_samples: 16_000 * 2,
        max_utterance_samples: 16_000 * 12,
    });

    let inbox = Arc::new(Mutex::new(SttJobInbox::default()));
    let (wake_tx, wake_rx) = bounded::<()>(1);
    let (transcript_tx, transcript_rx) = bounded::<Transcription>(32);

    let worker = spawn_stt_worker(inbox.clone(), wake_rx, transcript_tx);

    let started_at = Instant::now();
    let duration = Duration::from_secs(120);

    while started_at.elapsed() < duration {
        if let Ok(frame) = handle.frames.recv_timeout(Duration::from_millis(100)) {
            rolling_buffer.push_frame(&frame);

            let vad_update = vad.push_frame(frame)?;

            let requests = snapshot_scheduler.push_vad_update(vad_update);

            for request in requests {
                if let Ok(samples) = rolling_buffer.slice(request.start_sample, request.end_sample)
                {
                    let job = snapshot_request_to_job(request, samples);

                    {
                        let mut inbox = inbox.lock().expect("STT inbox mutex poisoned");
                        inbox.push(job);
                    }

                    let _ = wake_tx.try_send(());
                }
            }
        }

        while let Ok(transcription) = transcript_rx.try_recv() {
            if !transcription.text.is_empty() {
                println!("{}", transcription.text);
            }
        }
    }

    drop(wake_tx);

    handle.stop()?;

    let _ = worker.join();

    Ok(())
}

fn snapshot_request_to_job(request: AudioSnapshotRequest, samples: Vec<f32>) -> SttJob {
    let kind = match request.kind {
        SnapshotKind::LivePartial => SttJobKind::LivePartial {
            utterance_id: request.utterance_id,
        },

        SnapshotKind::Final { reason } => {
            let _reason = match reason {
                FinalReason::Silence => "silence",
                FinalReason::MaxUtterance => "max_utterance",
            };

            SttJobKind::Final {
                utterance_id: request.utterance_id,
            }
        }
    };

    SttJob::new(kind, request.start_sample, request.end_sample, samples)
}

fn spawn_stt_worker(
    inbox: Arc<Mutex<SttJobInbox>>,
    wake_rx: Receiver<()>,
    transcript_tx: Sender<Transcription>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let stt = match WhisperEngine::new(SttConfig {
            model_path: whisper_model_path("ggml-medium.en-q5_0.bin"),
            single_segment: true,
            ..Default::default()
        }) {
            Ok(stt) => stt,
            Err(err) => {
                eprintln!("failed to initialize STT: {err:?}");
                return;
            }
        };

        while wake_rx.recv().is_ok() {
            loop {
                let job = {
                    let mut inbox = inbox.lock().expect("STT inbox mutex poisoned");
                    inbox.pop_next()
                };

                let Some(job) = job else {
                    break;
                };

                let started = Instant::now();

                match stt.transcribe_samples(&job.samples) {
                    Ok(transcription) => {
                        let audio_secs = job.duration_seconds();
                        let stt_secs = started.elapsed().as_secs_f32();
                        let rtf = if audio_secs > 0.0 {
                            stt_secs / audio_secs
                        } else {
                            0.0
                        };

                        eprintln!(
                            "stt_job={:?} audio={:.2}s stt={:.2}s rtf={:.2}",
                            job.kind, audio_secs, stt_secs, rtf
                        );

                        let _ = transcript_tx.try_send(transcription);
                    }
                    Err(err) => {
                        eprintln!("STT failed: {err:?}");
                    }
                }
            }
        }
    })
}
