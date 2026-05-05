use std::collections::VecDeque;

use tracing::{debug, trace};

use crate::{SttJob, SttJobKind};

#[derive(Debug, Default)]
pub struct SttJobInbox {
    finals: VecDeque<SttJob>,
    recoveries: VecDeque<SttJob>,
    archives: VecDeque<SttJob>,

    /// Only the latest live partial matters.
    latest_partial: Option<SttJob>,

    /// Once an utterance has a final job queued/processed,
    /// partials for that utterance or older are stale.
    closed_utterance_up_to: u64,
}

impl SttJobInbox {
    pub fn push(&mut self, job: SttJob) {
        let pending_before = self.pending_count();

        match job.kind {
            SttJobKind::LivePartial { utterance_id } => {
                if utterance_id <= self.closed_utterance_up_to {
                    debug!(
                        ?job.kind,
                        start_sample = job.start_sample,
                        end_sample = job.end_sample,
                        closed_utterance_up_to = self.closed_utterance_up_to,
                        "dropping stale live partial"
                    );

                    return;
                }

                if let Some(previous) = &self.latest_partial {
                    debug!(
                        previous_kind = ?previous.kind,
                        previous_range = ?(previous.start_sample, previous.end_sample),
                        new_kind = ?job.kind,
                        new_range = ?(job.start_sample, job.end_sample),
                        "replacing pending live partial"
                    );
                }

                self.latest_partial = Some(job);
            }

            SttJobKind::Final { utterance_id } => {
                self.closed_utterance_up_to = self.closed_utterance_up_to.max(utterance_id);

                if let Some(partial) = &self.latest_partial {
                    if partial_utterance_id(partial) <= Some(utterance_id) {
                        debug!(
                            final_utterance_id = utterance_id,
                            partial_kind = ?partial.kind,
                            partial_range = ?(partial.start_sample, partial.end_sample),
                            "dropping pending partial because final arrived"
                        );

                        self.latest_partial = None;
                    }
                }

                self.finals.push_back(job);
            }

            SttJobKind::Recovery { .. } => {
                self.recoveries.push_back(job);
            }

            SttJobKind::Archive => {
                self.archives.push_back(job);
            }
        }

        trace!(
            pending_before,
            pending_after = self.pending_count(),
            finals = self.finals.len(),
            recoveries = self.recoveries.len(),
            archives = self.archives.len(),
            has_partial = self.latest_partial.is_some(),
            "STT inbox push completed"
        );
    }

    pub fn pop_next(&mut self) -> Option<SttJob> {
        let job = self
            .finals
            .pop_front()
            .or_else(|| self.recoveries.pop_front())
            .or_else(|| self.latest_partial.take())
            .or_else(|| self.archives.pop_front());

        if let Some(job) = &job {
            debug!(
                ?job.kind,
                start_sample = job.start_sample,
                end_sample = job.end_sample,
                samples = job.sample_len(),
                audio_secs = job.duration_seconds(),
                queue_age_secs = job.queue_age_seconds(),
                pending_after_pop = self.pending_count(),
                "popped STT job"
            );
        }

        job
    }

    pub fn is_empty(&self) -> bool {
        self.finals.is_empty()
            && self.recoveries.is_empty()
            && self.latest_partial.is_none()
            && self.archives.is_empty()
    }

    pub fn pending_count(&self) -> usize {
        self.finals.len()
            + self.recoveries.len()
            + self.archives.len()
            + usize::from(self.latest_partial.is_some())
    }
}

fn partial_utterance_id(job: &SttJob) -> Option<u64> {
    match job.kind {
        SttJobKind::LivePartial { utterance_id } => Some(utterance_id),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RecoveryReason, SttJobKind};

    fn job(kind: SttJobKind, start: u64, end: u64) -> SttJob {
        SttJob::new(kind, start, end, vec![0.0; (end - start) as usize])
    }

    #[test]
    fn latest_partial_replaces_old_partial() {
        let mut inbox = SttJobInbox::default();

        inbox.push(job(SttJobKind::LivePartial { utterance_id: 1 }, 0, 10));
        inbox.push(job(SttJobKind::LivePartial { utterance_id: 1 }, 0, 20));
        inbox.push(job(SttJobKind::LivePartial { utterance_id: 1 }, 0, 30));

        assert_eq!(inbox.pending_count(), 1);

        let next = inbox.pop_next().unwrap();

        assert_eq!(next.start_sample, 0);
        assert_eq!(next.end_sample, 30);
    }

    #[test]
    fn final_drops_pending_partial_for_same_utterance() {
        let mut inbox = SttJobInbox::default();

        inbox.push(job(SttJobKind::LivePartial { utterance_id: 10 }, 0, 20));
        inbox.push(job(SttJobKind::Final { utterance_id: 10 }, 0, 30));

        assert_eq!(inbox.pending_count(), 1);

        let next = inbox.pop_next().unwrap();
        assert_eq!(next.kind, SttJobKind::Final { utterance_id: 10 });

        assert!(inbox.pop_next().is_none());
    }

    #[test]
    fn stale_partial_after_final_is_ignored() {
        let mut inbox = SttJobInbox::default();

        inbox.push(job(SttJobKind::Final { utterance_id: 10 }, 0, 30));
        inbox.push(job(SttJobKind::LivePartial { utterance_id: 10 }, 0, 20));

        assert_eq!(inbox.pending_count(), 1);

        let next = inbox.pop_next().unwrap();
        assert_eq!(next.kind, SttJobKind::Final { utterance_id: 10 });

        assert!(inbox.pop_next().is_none());
    }

    #[test]
    fn newer_partial_after_final_is_allowed() {
        let mut inbox = SttJobInbox::default();

        inbox.push(job(SttJobKind::Final { utterance_id: 10 }, 0, 30));
        inbox.push(job(SttJobKind::LivePartial { utterance_id: 11 }, 30, 50));

        assert_eq!(inbox.pending_count(), 2);

        assert_eq!(
            inbox.pop_next().unwrap().kind,
            SttJobKind::Final { utterance_id: 10 }
        );

        assert_eq!(
            inbox.pop_next().unwrap().kind,
            SttJobKind::LivePartial { utterance_id: 11 }
        );
    }

    #[test]
    fn recovery_has_priority_over_partial_but_not_final() {
        let mut inbox = SttJobInbox::default();

        inbox.push(job(SttJobKind::LivePartial { utterance_id: 1 }, 0, 20));
        inbox.push(job(
            SttJobKind::Recovery {
                reason: RecoveryReason::MissingNeighbor,
            },
            20,
            40,
        ));
        inbox.push(job(SttJobKind::Final { utterance_id: 1 }, 0, 50));

        assert!(matches!(
            inbox.pop_next().unwrap().kind,
            SttJobKind::Final { .. }
        ));

        assert!(matches!(
            inbox.pop_next().unwrap().kind,
            SttJobKind::Recovery { .. }
        ));

        assert!(inbox.pop_next().is_none());
    }
}
