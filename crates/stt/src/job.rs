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
}

impl SttJob {
    pub fn new(kind: SttJobKind, start_sample: u64, end_sample: u64, samples: Vec<f32>) -> Self {
        Self {
            kind,
            start_sample,
            end_sample,
            samples,
        }
    }

    pub fn duration_seconds(&self) -> f32 {
        self.samples.len() as f32 / 16_000.0
    }

    pub fn is_partial(&self) -> bool {
        matches!(self.kind, SttJobKind::LivePartial { .. })
    }
}
