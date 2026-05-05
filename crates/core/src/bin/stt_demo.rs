use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use audio::{
    AudioSnapshotRequest, CaptureDevice, CpalCapture, CpalCaptureConfig, FinalReason,
    LiveSnapshotScheduler, LiveSnapshotSchedulerConfig, RollingAudioBuffer, SileroVadConfig,
    SileroVadWatcher, SnapshotKind,
};
use crossbeam_channel::{Receiver, Sender, bounded};
use stt::{
    SttConfig, SttJob, SttJobInbox, SttJobKind, Transcription, WhisperEngine, whisper_model_path,
};
use tracing::{debug, error, info, warn};

const SAMPLE_RATE: u32 = 16_000;
const FRAME_SAMPLES: usize = 512;

fn main() -> Result<()> {
    init_tracing();

    let running = Arc::new(AtomicBool::new(true));

    {
        let running = running.clone();

        ctrlc::set_handler(move || {
            running.store(false, Ordering::SeqCst);
        })
        .context("failed to install Ctrl+C handler")?;
    }

    info!("Tayori STT demo starting");

    let capture = CpalCapture::new(CpalCaptureConfig {
        device: CaptureDevice::DefaultMonitor,
        target_sample_rate: SAMPLE_RATE,
        ring_seconds: 10,
        output_frame_ms: 32,
        frame_channel_capacity: 512,
    });

    let capture_start = Instant::now();
    let handle = capture.start()?;

    info!(
        capture_start_secs = capture_start.elapsed().as_secs_f32(),
        "audio capture started"
    );

    let mut rolling_buffer = RollingAudioBuffer::with_seconds(60, SAMPLE_RATE);

    let vad_start = Instant::now();

    let mut vad = SileroVadWatcher::new(SileroVadConfig {
        threshold: 0.55,
        frame_samples: FRAME_SAMPLES,
        ..Default::default()
    })?;

    info!(
        vad_init_secs = vad_start.elapsed().as_secs_f32(),
        "Silero VAD initialized"
    );

    let mut snapshot_scheduler = LiveSnapshotScheduler::new(LiveSnapshotSchedulerConfig {
        min_transcribe_samples: SAMPLE_RATE as usize,
        partial_interval_samples: SAMPLE_RATE as usize * 2,
        max_utterance_samples: SAMPLE_RATE as usize * 6,
    });

    let inbox = Arc::new(Mutex::new(SttJobInbox::default()));
    let (wake_tx, wake_rx) = bounded::<()>(1);
    let (transcript_tx, transcript_rx) = bounded::<Transcription>(32);

    let worker = spawn_stt_worker(inbox.clone(), wake_rx, transcript_tx);

    let started_at = Instant::now();

    let mut frames_seen = 0_u64;
    let mut jobs_created = 0_u64;
    let mut last_frame_report = Instant::now();

    while running.load(Ordering::SeqCst) {
        let recv_started = Instant::now();

        if let Ok(frame) = handle.frames.recv_timeout(Duration::from_millis(100)) {
            let recv_secs = recv_started.elapsed().as_secs_f32();

            frames_seen += 1;

            let frame_samples = frame.samples.len();

            if recv_secs > 0.050 {
                warn!(
                    recv_secs,
                    frames_seen, frame_samples, "audio frame receive waited longer than expected"
                );
            }

            let push_started = Instant::now();
            let span = rolling_buffer.push_frame(&frame);
            let push_secs = push_started.elapsed().as_secs_f32();

            if push_secs > 0.005 {
                warn!(
                    push_secs,
                    start_sample = span.start_sample,
                    end_sample = span.end_sample,
                    "rolling buffer push was slow"
                );
            }

            let vad_started = Instant::now();
            let vad_update = vad.push_frame(frame)?;
            let vad_secs = vad_started.elapsed().as_secs_f32();

            if vad_secs > 0.010 {
                warn!(
                    vad_secs,
                    transition = ?vad_update.transition,
                    speech_active = vad_update.speech_active,
                    "VAD processing was slow"
                );
            }

            let schedule_started = Instant::now();
            let requests = snapshot_scheduler.push_vad_update(vad_update);
            let schedule_secs = schedule_started.elapsed().as_secs_f32();

            if schedule_secs > 0.005 {
                warn!(
                    schedule_secs,
                    requests = requests.len(),
                    "snapshot scheduler was slow"
                );
            }

            for request in requests {
                let slice_started = Instant::now();

                let Ok(samples) = rolling_buffer.slice(request.start_sample, request.end_sample)
                else {
                    warn!(
                        utterance_id = request.utterance_id,
                        kind = ?request.kind,
                        start_sample = request.start_sample,
                        end_sample = request.end_sample,
                        "failed to slice rolling audio buffer for STT job"
                    );

                    continue;
                };

                let slice_secs = slice_started.elapsed().as_secs_f32();

                if slice_secs > 0.010 {
                    warn!(
                        slice_secs,
                        samples = samples.len(),
                        utterance_id = request.utterance_id,
                        kind = ?request.kind,
                        "rolling buffer slice/copy was slow"
                    );
                } else {
                    debug!(
                        slice_secs,
                        samples = samples.len(),
                        utterance_id = request.utterance_id,
                        kind = ?request.kind,
                        "rolling buffer slice/copy completed"
                    );
                }

                let job = snapshot_request_to_job(request, samples);
                let job_kind = job.kind.clone();
                let job_samples = job.samples.len();
                let job_audio_secs = job.duration_seconds();
                let job_range = (job.start_sample, job.end_sample);

                let inbox_started = Instant::now();

                let pending_after_push = {
                    let mut inbox = inbox.lock().expect("STT inbox mutex poisoned");
                    inbox.push(job);
                    inbox.pending_count()
                };

                let inbox_secs = inbox_started.elapsed().as_secs_f32();

                jobs_created += 1;

                if inbox_secs > 0.005 {
                    warn!(
                        inbox_secs,
                        ?job_kind,
                        samples = job_samples,
                        audio_secs = job_audio_secs,
                        range = ?job_range,
                        pending_after_push,
                        "STT inbox push was slow"
                    );
                } else {
                    debug!(
                        inbox_secs,
                        ?job_kind,
                        samples = job_samples,
                        audio_secs = job_audio_secs,
                        range = ?job_range,
                        pending_after_push,
                        "STT job queued"
                    );
                }

                let wake_started = Instant::now();
                let wake_result = wake_tx.try_send(());
                let wake_secs = wake_started.elapsed().as_secs_f32();

                if wake_secs > 0.005 {
                    warn!(wake_secs, ?wake_result, "wake signal send was slow");
                }

                if wake_result.is_err() {
                    debug!("STT worker already has a wake signal pending");
                }
            }

            if !is_quite_mode() && last_frame_report.elapsed() >= Duration::from_secs(1) {
                let pending = {
                    let inbox = inbox.lock().expect("STT inbox mutex poisoned");
                    inbox.pending_count()
                };

                info!(
                    elapsed_secs = started_at.elapsed().as_secs_f32(),
                    frames_seen,
                    jobs_created,
                    pending_jobs = pending,
                    rolling_base = rolling_buffer.base_sample_index(),
                    rolling_next = rolling_buffer.next_sample_index(),
                    "pipeline heartbeat"
                );

                last_frame_report = Instant::now();
            }
        }

        while let Ok(transcription) = transcript_rx.try_recv() {
            if !transcription.text.trim().is_empty() {
                println!("{}", transcription.text.trim());
            }
        }
    }

    info!("Ctrl+C received; stopping STT demo");

    drop(wake_tx);

    let stop_started = Instant::now();
    handle.stop()?;

    info!(
        capture_stop_secs = stop_started.elapsed().as_secs_f32(),
        "audio capture stopped"
    );

    let _ = worker.join();

    info!(
        total_secs = started_at.elapsed().as_secs_f32(),
        frames_seen, jobs_created, "Tayori STT demo finished"
    );

    Ok(())
}

fn is_quite_mode() -> bool {
    matches!(std::env::var("QUIET").as_deref(), Ok("1" | "true"))
}

fn init_tracing() {
    if is_quite_mode() {
        return;
    }

    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_target(false)
        .with_level(true)
        .with_max_level(tracing::Level::INFO)
        .init();
}

fn snapshot_request_to_job(request: AudioSnapshotRequest, samples: Vec<f32>) -> SttJob {
    let kind = match request.kind {
        SnapshotKind::LivePartial => SttJobKind::LivePartial {
            utterance_id: request.utterance_id,
        },

        SnapshotKind::Final { reason } => {
            let reason_label = match reason {
                FinalReason::Silence => "silence",
                FinalReason::MaxUtterance => "max_utterance",
            };

            debug!(
                utterance_id = request.utterance_id,
                reason = reason_label,
                start_sample = request.start_sample,
                end_sample = request.end_sample,
                "creating final STT job"
            );

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
        info!("STT worker starting");

        let init_started = Instant::now();

        let stt = match WhisperEngine::new(SttConfig {
            model_path: whisper_model_path("ggml-small-q8_0.bin"),
            single_segment: true,
            ..Default::default()
        }) {
            Ok(stt) => stt,
            Err(err) => {
                error!(?err, "failed to initialize STT");
                return;
            }
        };

        info!(
            stt_init_secs = init_started.elapsed().as_secs_f32(),
            "STT worker initialized"
        );

        while wake_rx.recv().is_ok() {
            debug!("STT worker woke up");

            loop {
                let pop_started = Instant::now();

                let job = {
                    let mut inbox = inbox.lock().expect("STT inbox mutex poisoned");
                    inbox.pop_next()
                };

                let pop_secs = pop_started.elapsed().as_secs_f32();

                if pop_secs > 0.005 {
                    warn!(pop_secs, "STT inbox pop was slow");
                }

                let Some(job) = job else {
                    debug!("STT worker inbox drained");
                    break;
                };

                let job_started = Instant::now();

                let audio_secs = job.duration_seconds();
                let queue_age_secs = job.queue_age_seconds();

                info!(
                    ?job.kind,
                    start_sample = job.start_sample,
                    end_sample = job.end_sample,
                    samples = job.samples.len(),
                    audio_secs,
                    queue_age_secs,
                    "STT job started"
                );

                match stt.transcribe_samples(&job.samples) {
                    Ok(transcription) => {
                        let stt_secs = job_started.elapsed().as_secs_f32();
                        let rtf = if audio_secs > 0.0 {
                            stt_secs / audio_secs
                        } else {
                            0.0
                        };

                        info!(
                            ?job.kind,
                            audio_secs,
                            stt_secs,
                            rtf,
                            queue_age_secs,
                            text_len = transcription.text.len(),
                            segments = transcription.segments.len(),
                            "STT job completed"
                        );

                        if stt_secs > 1.0 {
                            warn!(
                                ?job.kind,
                                audio_secs,
                                stt_secs,
                                rtf,
                                queue_age_secs,
                                "STT job exceeded 1s"
                            );
                        }

                        let send_started = Instant::now();
                        let send_result = transcript_tx.try_send(transcription);
                        let send_secs = send_started.elapsed().as_secs_f32();

                        if send_secs > 0.005 {
                            warn!(send_secs, ?send_result, "transcript send was slow");
                        }

                        if send_result.is_err() {
                            warn!("transcript channel full/disconnected; dropping transcript");
                        }
                    }

                    Err(err) => {
                        error!(?err, ?job.kind, "STT failed");
                    }
                }
            }
        }

        info!("STT worker exiting");
    })
}
