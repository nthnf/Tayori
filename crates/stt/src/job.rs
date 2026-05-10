/// Raw audio job handed to STT.
#[derive(Clone, Debug, PartialEq)]
pub struct TranscriptionJob {
    /// Monotonic per-stream chunk index.
    pub index: u64,
    /// 16 kHz mono PCM samples.
    pub samples: Vec<f32>,
}

impl TranscriptionJob {
    /// Build a transcription job from raw samples.
    pub fn new(index: u64, samples: Vec<f32>) -> Self {
        Self { index, samples }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_constructor_keeps_samples_and_index() {
        let job = TranscriptionJob::new(3, vec![0.1, 0.2, 0.3]);

        assert_eq!(job.index, 3);
        assert_eq!(job.samples, vec![0.1, 0.2, 0.3]);
    }
}
