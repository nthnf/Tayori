#[derive(Debug, Clone)]
pub struct SileroVadConfig {
    /// Silero speech probability threshold.
    ///
    /// Higher = stricter, fewer false positives.
    /// Lower = more sensitive, catches quieter speech.
    pub threshold: f32,

    /// Expected frame size.
    ///
    /// At 16 kHz:
    /// 512 samples = 32 ms.
    pub frame_samples: usize,

    /// Minimum segment duration before emitting speech.
    pub min_segment_ms: usize,

    /// Maximum segment duration.
    ///
    /// This prevents infinite buffering if the other party has a broken/buzzing mic
    /// and VAD never emits a clean end.
    pub max_segment_ms: usize,

    /// Number of previous non-speech frames to prepend when speech starts.
    ///
    /// This prevents cutting off the first syllable.
    pub pre_roll_frames: usize,

    /// If true, allow a forced segment when max_segment_ms is reached.
    pub force_emit_on_max_duration: bool,
}

impl Default for SileroVadConfig {
    fn default() -> Self {
        Self {
            threshold: 0.55,
            frame_samples: 512,
            min_segment_ms: 1_000,
            max_segment_ms: 8_000,
            pre_roll_frames: 3,
            force_emit_on_max_duration: true,
        }
    }
}

impl SileroVadConfig {
    pub fn min_segment_samples(&self, sample_rate: u32) -> usize {
        sample_rate as usize * self.min_segment_ms / 1000
    }

    pub fn max_segment_samples(&self, sample_rate: u32) -> usize {
        sample_rate as usize * self.max_segment_ms / 1000
    }
}
