/// Final speech slice from VAD.
///
/// This is not 100ms transport frame. It is finalized speech segment ready for
/// Whisper, storage, and later retrieval.
#[derive(Clone)]
pub struct SpeechSegment {
    /// 16kHz mono f32 samples for one finalized speech segment.
    pub samples: Vec<f32>,
    /// Monotonic per-stream segment index (0, 1, 2, ...).
    pub index: u64,
    /// Stream-relative segment start time.
    pub start_ms: u64,
    /// Stream-relative segment end time.
    pub end_ms: u64,
    /// Segment speech duration.
    pub duration_ms: u64,
}

impl SpeechSegment {
    /// Build finalized speech segment.
    pub fn new(samples: Vec<f32>, index: u64, start_ms: u64, end_ms: u64) -> Self {
        Self {
            samples,
            index,
            start_ms,
            end_ms,
            duration_ms: end_ms.saturating_sub(start_ms),
        }
    }
}
