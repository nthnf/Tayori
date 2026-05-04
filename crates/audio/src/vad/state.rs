#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VadSignal {
    None,
    SpeechStart,
    SpeechEnd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeechTransition {
    None,
    Started,
    Ended,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VadFrameUpdate {
    pub start_sample: u64,
    pub end_sample: u64,
    pub speech_active: bool,
    pub utterance_start_sample: Option<u64>,
    pub transition: SpeechTransition,
}

#[derive(Debug, Clone)]
pub struct VadStateTracker {
    next_sample_index: u64,
    speech_active: bool,
    utterance_start_sample: Option<u64>,
}

impl Default for VadStateTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl VadStateTracker {
    pub fn new() -> Self {
        Self {
            next_sample_index: 0,
            speech_active: false,
            utterance_start_sample: None,
        }
    }

    pub fn push_frame(&mut self, frame_samples: usize, signal: VadSignal) -> VadFrameUpdate {
        let start_sample = self.next_sample_index;
        let end_sample = start_sample + frame_samples as u64;

        self.next_sample_index = end_sample;

        match signal {
            VadSignal::SpeechStart => {
                self.speech_active = true;

                if self.utterance_start_sample.is_none() {
                    self.utterance_start_sample = Some(start_sample);
                }

                VadFrameUpdate {
                    start_sample,
                    end_sample,
                    speech_active: true,
                    utterance_start_sample: self.utterance_start_sample,
                    transition: SpeechTransition::Started,
                }
            }

            VadSignal::SpeechEnd => {
                let utterance_start_sample = self.utterance_start_sample;

                self.speech_active = false;
                self.utterance_start_sample = None;

                VadFrameUpdate {
                    start_sample,
                    end_sample,
                    speech_active: false,
                    utterance_start_sample,
                    transition: SpeechTransition::Ended,
                }
            }

            VadSignal::None => VadFrameUpdate {
                start_sample,
                end_sample,
                speech_active: self.speech_active,
                utterance_start_sample: self.utterance_start_sample,
                transition: SpeechTransition::None,
            },
        }
    }

    pub fn next_sample_index(&self) -> u64 {
        self.next_sample_index
    }

    pub fn speech_active(&self) -> bool {
        self.speech_active
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_speech_start_and_end() {
        let mut tracker = VadStateTracker::new();

        let update = tracker.push_frame(512, VadSignal::None);
        assert_eq!(update.speech_active, false);
        assert_eq!(update.transition, SpeechTransition::None);

        let update = tracker.push_frame(512, VadSignal::SpeechStart);
        assert_eq!(update.start_sample, 512);
        assert_eq!(update.end_sample, 1024);
        assert_eq!(update.speech_active, true);
        assert_eq!(update.utterance_start_sample, Some(512));
        assert_eq!(update.transition, SpeechTransition::Started);

        let update = tracker.push_frame(512, VadSignal::None);
        assert_eq!(update.speech_active, true);
        assert_eq!(update.utterance_start_sample, Some(512));

        let update = tracker.push_frame(512, VadSignal::SpeechEnd);
        assert_eq!(update.speech_active, false);
        assert_eq!(update.utterance_start_sample, Some(512));
        assert_eq!(update.transition, SpeechTransition::Ended);

        let update = tracker.push_frame(512, VadSignal::None);
        assert_eq!(update.speech_active, false);
        assert_eq!(update.utterance_start_sample, None);
    }
}
