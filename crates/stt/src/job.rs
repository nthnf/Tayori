use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryReason {
    NearCurrentQuestion,
    MissingNeighbor,
    ExpiringSoon,
    IdleBackfill,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SttJobKind {
    LivePartial { utterance_id: u64 },
    Final { utterance_id: u64 },
    Recovery { reason: RecoveryReason },
    Archive,
}

#[derive(Debug, Clone)]
pub struct SttJob {
    pub kind: SttJobKind,
    pub start_sample: u64,
    pub end_sample: u64,
    pub samples: Vec<f32>,

    /// When the scheduler created this job.
    ///
    /// Used to measure queue delay before the STT worker starts processing it.
    created_at: Instant,
}

impl SttJob {
    pub fn new(kind: SttJobKind, start_sample: u64, end_sample: u64, samples: Vec<f32>) -> Self {
        Self {
            kind,
            start_sample,
            end_sample,
            samples,
            created_at: Instant::now(),
        }
    }

    pub fn duration_seconds(&self) -> f32 {
        self.samples.len() as f32 / 16_000.0
    }

    pub fn is_partial(&self) -> bool {
        matches!(self.kind, SttJobKind::LivePartial { .. })
    }

    pub fn queue_age(&self) -> Duration {
        self.created_at.elapsed()
    }

    pub fn queue_age_seconds(&self) -> f32 {
        self.queue_age().as_secs_f32()
    }

    pub fn sample_len(&self) -> usize {
        self.samples.len()
    }
}
