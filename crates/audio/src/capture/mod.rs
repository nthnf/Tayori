mod config;
mod cpal_backend;
mod device;
mod handle;
mod resampler;
mod ring_reader;
mod sample_convert;

pub use config::CpalCaptureConfig;
pub use device::{AudioDeviceInfo, CaptureDevice, list_audio_devices};
pub use handle::CpalCaptureHandle;

use anyhow::Result;

pub struct CpalCapture {
    config: CpalCaptureConfig,
}

impl CpalCapture {
    pub fn new(config: CpalCaptureConfig) -> Self {
        Self { config }
    }

    pub fn start(self) -> Result<CpalCaptureHandle> {
        cpal_backend::start_cpal_capture(self.config)
    }
}
