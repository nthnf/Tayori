use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

use anyhow::Result;
use crossbeam_channel::Receiver;
use tracing::info;

use crate::AudioFrame;

pub struct CpalCaptureHandle {
    pub frames: Receiver<AudioFrame>,

    pub(crate) stop: Arc<AtomicBool>,
    pub(crate) stream: Option<cpal::Stream>,
    pub(crate) reader_thread: Option<thread::JoinHandle<()>>,
}

impl CpalCaptureHandle {
    pub(crate) fn new(
        frames: Receiver<AudioFrame>,
        stop: Arc<AtomicBool>,
        stream: cpal::Stream,
        reader_thread: thread::JoinHandle<()>,
    ) -> Self {
        Self {
            frames,
            stop,
            stream: Some(stream),
            reader_thread: Some(reader_thread),
        }
    }

    pub fn stop(mut self) -> Result<()> {
        self.stop_inner();
        Ok(())
    }

    fn stop_inner(&mut self) {
        info!("stopping CPAL capture");

        self.stop.store(true, Ordering::Relaxed);

        // Dropping the CPAL stream stops the audio callback.
        self.stream.take();

        if let Some(handle) = self.reader_thread.take() {
            let _ = handle.join();
        }

        info!("CPAL capture stopped");
    }
}

impl Drop for CpalCaptureHandle {
    fn drop(&mut self) {
        self.stop_inner();
    }
}
