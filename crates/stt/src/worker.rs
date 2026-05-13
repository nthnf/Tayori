use anyhow::Result;
use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};
use tracing::debug;

use crate::{chunk::TranscriptionChunk, job::TranscriptionJob, transcribe::Transcriptor};

/// Background worker that turns raw PCM jobs into transcription chunks.
pub struct TranscriptionWorker {
    input: Option<Receiver<TranscriptionJob>>,
    output: Sender<TranscriptionChunk>,
    transcriptor: Option<Transcriptor>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl TranscriptionWorker {
    /// Build transcription worker from job queue and output channel.
    ///
    /// The worker owns the Whisper wrapper and runs on a dedicated thread until
    /// the shared stop flag is set or the job queue disconnects.
    pub fn new(
        model_name: &str,
        input: Receiver<TranscriptionJob>,
        output: Sender<TranscriptionChunk>,
        stop: Arc<AtomicBool>,
    ) -> Result<Self> {
        // Load Whisper once when building the worker. Model/context setup is
        // expensive, so jobs reuse this transcriptor instead of reloading files.
        let transcriptor = Transcriptor::new(model_name)?;

        Ok(Self {
            input: Some(input),
            output,
            transcriptor: Some(transcriptor),
            stop,
            worker: None,
        })
    }

    /// Start transcription worker thread.
    pub fn start(&mut self) -> Result<()> {
        anyhow::ensure!(
            self.worker.is_none(),
            "transcription worker already started"
        );

        // Move queue endpoints and Whisper wrapper into the background thread.
        // Taking the Options also prevents accidentally starting this worker twice.
        let input = self
            .input
            .take()
            .ok_or_else(|| anyhow::anyhow!("transcription input already taken"))?;
        let output = self.output.clone();
        let stop = self.stop.clone();
        let transcriptor = self
            .transcriptor
            .take()
            .ok_or_else(|| anyhow::anyhow!("transcriptor already taken"))?;

        self.worker = Some(thread::spawn(move || {
            drain_jobs(input, output, transcriptor, stop);
        }));

        Ok(())
    }

    /// Stop worker thread.
    pub fn stop(&mut self) {
        // Do not join here. Whisper decoding can be in progress and may take
        // long enough to freeze the UI during session End/Pause. Dropping the
        // handle detaches the worker; the shared stop flag makes it exit after
        // the current decode finishes.
        let _ = self.worker.take();
    }
}

fn drain_jobs(
    input: Receiver<TranscriptionJob>,
    output: Sender<TranscriptionChunk>,
    transcriptor: Transcriptor,
    stop: Arc<AtomicBool>,
) {
    let poll = Duration::from_millis(10);

    loop {
        // Wait briefly for a job, but wake often enough to notice shutdown.
        let job = match input.recv_timeout(poll) {
            Ok(job) => job,
            Err(RecvTimeoutError::Timeout) => {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => break,
        };
        handle_job(&transcriptor, &output, job);

        // Drain any backlog immediately. This avoids one poll interval per job
        // when VAD emitted multiple segments while Whisper was busy.
        while let Ok(job) = input.try_recv() {
            handle_job(&transcriptor, &output, job);
        }

        if stop.load(Ordering::Relaxed) {
            break;
        }
    }
}

fn handle_job(
    transcriptor: &Transcriptor,
    output: &Sender<TranscriptionChunk>,
    job: TranscriptionJob,
) {
    let started_at = Instant::now();
    // Job samples are already 16 kHz mono from VAD. The worker only measures
    // queue-to-output latency and delegates actual decoding to Transcriptor.
    debug!(
        index = job.index,
        samples = job.samples.len(),
        audio_ms = job.samples.len() as f64 / 16_000.0 * 1_000.0,
        "STT job started"
    );

    match transcriptor.transcribe(&job.samples) {
        Ok(body) => {
            let transcribe_ms = started_at.elapsed().as_secs_f64() * 1_000.0;
            debug!(
                index = job.index,
                text_len = body.len(),
                transcribe_ms,
                "STT job finished; sending transcript"
            );
            // If the UI/demo receiver is gone, drop the transcript. The pipeline
            // is shutting down or nobody is listening anymore.
            let _ = output.send(TranscriptionChunk::new(
                body,
                job.index,
                job.start_ms,
                job.end_ms,
            ));
            debug!(
                index = job.index,
                total_ms = started_at.elapsed().as_secs_f64() * 1_000.0,
                "transcript queued"
            );
        }
        Err(err) => tracing::error!(?err, index = job.index, "transcription failed"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_without_start_is_noop() {
        let (_job_tx, job_rx) = crossbeam_channel::unbounded();
        let (chunk_tx, _chunk_rx) = crossbeam_channel::unbounded();
        let stop = Arc::new(AtomicBool::new(false));

        let worker = TranscriptionWorker {
            input: Some(job_rx),
            output: chunk_tx,
            transcriptor: None,
            stop,
            worker: None,
        };

        let mut worker = worker;
        worker.stop();
    }
}
