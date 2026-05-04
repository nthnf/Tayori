use crate::{SpeechTransition, VadFrameUpdate};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotKind {
    LivePartial,
    Final { reason: FinalReason },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinalReason {
    Silence,
    MaxUtterance,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioSnapshotRequest {
    pub utterance_id: u64,
    pub kind: SnapshotKind,
    pub start_sample: u64,
    pub end_sample: u64,
}

#[derive(Debug, Clone)]
pub struct LiveSnapshotSchedulerConfig {
    pub min_transcribe_samples: usize,
    pub partial_interval_samples: usize,
    pub max_utterance_samples: usize,
}

impl Default for LiveSnapshotSchedulerConfig {
    fn default() -> Self {
        Self {
            // 1 second
            min_transcribe_samples: 16_000,

            // 2 seconds
            partial_interval_samples: 16_000 * 2,

            // 6 seconds
            max_utterance_samples: 16_000 * 6,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LiveSnapshotScheduler {
    config: LiveSnapshotSchedulerConfig,
    utterance_id: u64,
    active: bool,
    utterance_start_sample: Option<u64>,
    last_partial_sample: Option<u64>,
}

impl LiveSnapshotScheduler {
    pub fn new(config: LiveSnapshotSchedulerConfig) -> Self {
        Self {
            config,
            utterance_id: 0,
            active: false,
            utterance_start_sample: None,
            last_partial_sample: None,
        }
    }

    pub fn with_default_config() -> Self {
        Self::new(LiveSnapshotSchedulerConfig::default())
    }

    pub fn push_vad_update(&mut self, update: VadFrameUpdate) -> Vec<AudioSnapshotRequest> {
        match update.transition {
            SpeechTransition::Started => {
                self.active = true;
                self.utterance_id += 1;
                self.utterance_start_sample = Some(update.start_sample);
                self.last_partial_sample = Some(update.start_sample);

                Vec::new()
            }

            SpeechTransition::Ended => self.finish_on_silence(update),

            SpeechTransition::None => {
                if update.speech_active {
                    self.maybe_emit_active_snapshot(update)
                } else {
                    Vec::new()
                }
            }
        }
    }

    fn maybe_emit_active_snapshot(&mut self, update: VadFrameUpdate) -> Vec<AudioSnapshotRequest> {
        let Some(start_sample) = self.utterance_start_sample else {
            return Vec::new();
        };

        let current_len = update.end_sample.saturating_sub(start_sample) as usize;

        if current_len < self.config.min_transcribe_samples {
            return Vec::new();
        }

        if current_len >= self.config.max_utterance_samples {
            let request = AudioSnapshotRequest {
                utterance_id: self.utterance_id,
                kind: SnapshotKind::Final {
                    reason: FinalReason::MaxUtterance,
                },
                start_sample,
                end_sample: update.end_sample,
            };

            // Continue active speech as a new utterance from this point.
            self.utterance_id += 1;
            self.active = true;
            self.utterance_start_sample = Some(update.end_sample);
            self.last_partial_sample = Some(update.end_sample);

            return vec![request];
        }

        let last_partial_sample = self.last_partial_sample.unwrap_or(start_sample);

        let since_last_partial = update.end_sample.saturating_sub(last_partial_sample) as usize;

        if since_last_partial < self.config.partial_interval_samples {
            return Vec::new();
        }

        self.last_partial_sample = Some(update.end_sample);

        vec![AudioSnapshotRequest {
            utterance_id: self.utterance_id,
            kind: SnapshotKind::LivePartial,
            start_sample,
            end_sample: update.end_sample,
        }]
    }

    fn finish_on_silence(&mut self, update: VadFrameUpdate) -> Vec<AudioSnapshotRequest> {
        let start_sample = update
            .utterance_start_sample
            .or(self.utterance_start_sample);

        self.active = false;
        self.utterance_start_sample = None;
        self.last_partial_sample = None;

        let Some(start_sample) = start_sample else {
            return Vec::new();
        };

        let len = update.end_sample.saturating_sub(start_sample) as usize;

        if len < self.config.min_transcribe_samples {
            return Vec::new();
        }

        vec![AudioSnapshotRequest {
            utterance_id: self.utterance_id,
            kind: SnapshotKind::Final {
                reason: FinalReason::Silence,
            },
            start_sample,
            end_sample: update.end_sample,
        }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SpeechTransition, VadFrameUpdate};

    fn update(
        start_sample: u64,
        end_sample: u64,
        speech_active: bool,
        utterance_start_sample: Option<u64>,
        transition: SpeechTransition,
    ) -> VadFrameUpdate {
        VadFrameUpdate {
            start_sample,
            end_sample,
            speech_active,
            utterance_start_sample,
            transition,
        }
    }

    #[test]
    fn emits_partial_after_interval() {
        let mut scheduler = LiveSnapshotScheduler::new(LiveSnapshotSchedulerConfig {
            min_transcribe_samples: 1_000,
            partial_interval_samples: 2_000,
            max_utterance_samples: 10_000,
        });

        assert!(
            scheduler
                .push_vad_update(update(0, 500, true, Some(0), SpeechTransition::Started))
                .is_empty()
        );

        assert!(
            scheduler
                .push_vad_update(update(500, 1_500, true, Some(0), SpeechTransition::None))
                .is_empty()
        );

        let requests =
            scheduler.push_vad_update(update(1_500, 2_500, true, Some(0), SpeechTransition::None));

        assert_eq!(
            requests,
            vec![AudioSnapshotRequest {
                utterance_id: 1,
                kind: SnapshotKind::LivePartial,
                start_sample: 0,
                end_sample: 2_500,
            }]
        );
    }

    #[test]
    fn emits_final_on_silence() {
        let mut scheduler = LiveSnapshotScheduler::new(LiveSnapshotSchedulerConfig {
            min_transcribe_samples: 1_000,
            partial_interval_samples: 2_000,
            max_utterance_samples: 10_000,
        });

        scheduler.push_vad_update(update(0, 500, true, Some(0), SpeechTransition::Started));

        let requests = scheduler.push_vad_update(update(
            2_000,
            2_500,
            false,
            Some(0),
            SpeechTransition::Ended,
        ));

        assert_eq!(
            requests,
            vec![AudioSnapshotRequest {
                utterance_id: 1,
                kind: SnapshotKind::Final {
                    reason: FinalReason::Silence
                },
                start_sample: 0,
                end_sample: 2_500,
            }]
        );
    }

    #[test]
    fn force_finalizes_when_max_utterance_is_reached() {
        let mut scheduler = LiveSnapshotScheduler::new(LiveSnapshotSchedulerConfig {
            min_transcribe_samples: 1_000,
            partial_interval_samples: 20_000,
            max_utterance_samples: 3_000,
        });

        scheduler.push_vad_update(update(0, 500, true, Some(0), SpeechTransition::Started));

        let requests =
            scheduler.push_vad_update(update(2_500, 3_500, true, Some(0), SpeechTransition::None));

        assert_eq!(
            requests,
            vec![AudioSnapshotRequest {
                utterance_id: 1,
                kind: SnapshotKind::Final {
                    reason: FinalReason::MaxUtterance
                },
                start_sample: 0,
                end_sample: 3_500,
            }]
        );

        let requests = scheduler.push_vad_update(update(
            3_500,
            5_000,
            true,
            Some(3_500),
            SpeechTransition::None,
        ));

        assert!(requests.is_empty());
    }
}
