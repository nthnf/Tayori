/// Raw audio job handed to STT.
#[derive(Clone, Debug, PartialEq)]
pub struct TranscriptionJob {
    /// Monotonic per-stream chunk index.
    pub index: u64,
    /// 16 kHz mono PCM samples.
    pub samples: Vec<f32>,
    /// Stream-relative segment start time.
    pub start_ms: u64,
    /// Stream-relative segment end time.
    pub end_ms: u64,
    /// Segment speech duration.
    pub duration_ms: u64,
}

impl TranscriptionJob {
    /// Build a transcription job from raw samples.
    pub fn new(index: u64, samples: Vec<f32>, start_ms: u64, end_ms: u64) -> Self {
        Self {
            index,
            samples,
            start_ms,
            end_ms,
            duration_ms: end_ms.saturating_sub(start_ms),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_constructor_keeps_samples_and_index() {
        let job = TranscriptionJob::new(3, vec![0.1, 0.2, 0.3], 100, 250);

        assert_eq!(job.index, 3);
        assert_eq!(job.samples, vec![0.1, 0.2, 0.3]);
        assert_eq!(job.start_ms, 100);
        assert_eq!(job.end_ms, 250);
        assert_eq!(job.duration_ms, 150);
    }
}
