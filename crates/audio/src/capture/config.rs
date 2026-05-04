use super::device::CaptureDevice;

#[derive(Debug, Clone)]
pub struct CpalCaptureConfig {
    /// Which CPAL device to try.
    ///
    /// For Tayori, DefaultMonitor is the correct default because the app wants
    /// meeting/system audio, not microphone audio.
    pub device: CaptureDevice,

    /// Tayori target sample rate.
    ///
    /// Whisper and Silero both want 16 kHz mono audio.
    pub target_sample_rate: u32,

    /// Ring buffer capacity in seconds.
    pub ring_seconds: usize,

    /// Output frame size after resampling.
    ///
    /// 32ms at 16kHz = 512 samples.
    pub output_frame_ms: usize,

    /// How many processed AudioFrame packets may wait before dropping.
    pub frame_channel_capacity: usize,
}

impl Default for CpalCaptureConfig {
    fn default() -> Self {
        Self {
            device: CaptureDevice::DefaultMonitor,
            target_sample_rate: 16_000,
            ring_seconds: 5,
            output_frame_ms: 32,
            frame_channel_capacity: 128,
        }
    }
}
