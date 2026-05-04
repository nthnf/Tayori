use std::{
    collections::VecDeque,
    error::Error,
    fmt::{self, Display, Formatter},
};

use crate::AudioFrame;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioFrameSpan {
    pub start_sample: u64,
    pub end_sample: u64,
}

impl AudioFrameSpan {
    pub fn len(&self) -> u64 {
        self.end_sample.saturating_sub(self.start_sample)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RollingBufferError {
    InvalidRange {
        start_sample: u64,
        end_sample: u64,
    },
    RangeExpired {
        requested_start: u64,
        available_start: u64,
    },
    RangeNotYetAvailable {
        requested_end: u64,
        available_end: u64,
    },
}

impl Display for RollingBufferError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRange {
                start_sample,
                end_sample,
            } => write!(
                f,
                "invalid audio range: start_sample={start_sample}, end_sample={end_sample}"
            ),
            Self::RangeExpired {
                requested_start,
                available_start,
            } => write!(
                f,
                "audio range expired: requested_start={requested_start}, available_start={available_start}"
            ),
            Self::RangeNotYetAvailable {
                requested_end,
                available_end,
            } => write!(
                f,
                "audio range not yet available: requested_end={requested_end}, available_end={available_end}"
            ),
        }
    }
}

impl Error for RollingBufferError {}

#[derive(Debug, Clone)]
pub struct RollingAudioBuffer {
    base_sample_index: u64,
    next_sample_index: u64,
    samples: VecDeque<f32>,
    max_samples: usize,
}

impl RollingAudioBuffer {
    pub fn new(max_samples: usize) -> Self {
        assert!(max_samples > 0, "rolling buffer max_samples must be > 0");

        Self {
            base_sample_index: 0,
            next_sample_index: 0,
            samples: VecDeque::with_capacity(max_samples),
            max_samples,
        }
    }

    pub fn with_seconds(seconds: usize, sample_rate: u32) -> Self {
        Self::new(seconds * sample_rate as usize)
    }

    pub fn push_frame(&mut self, frame: &AudioFrame) -> AudioFrameSpan {
        self.push_samples(&frame.samples)
    }

    pub fn push_samples(&mut self, samples: &[f32]) -> AudioFrameSpan {
        let start_sample = self.next_sample_index;

        self.samples.extend(samples.iter().copied());
        self.next_sample_index += samples.len() as u64;

        self.trim_to_capacity();

        AudioFrameSpan {
            start_sample,
            end_sample: self.next_sample_index,
        }
    }

    pub fn slice(
        &self,
        start_sample: u64,
        end_sample: u64,
    ) -> Result<Vec<f32>, RollingBufferError> {
        if start_sample >= end_sample {
            return Err(RollingBufferError::InvalidRange {
                start_sample,
                end_sample,
            });
        }

        if start_sample < self.base_sample_index {
            return Err(RollingBufferError::RangeExpired {
                requested_start: start_sample,
                available_start: self.base_sample_index,
            });
        }

        if end_sample > self.next_sample_index {
            return Err(RollingBufferError::RangeNotYetAvailable {
                requested_end: end_sample,
                available_end: self.next_sample_index,
            });
        }

        let local_start = (start_sample - self.base_sample_index) as usize;
        let local_end = (end_sample - self.base_sample_index) as usize;

        Ok(self
            .samples
            .iter()
            .skip(local_start)
            .take(local_end - local_start)
            .copied()
            .collect())
    }

    pub fn base_sample_index(&self) -> u64 {
        self.base_sample_index
    }

    pub fn next_sample_index(&self) -> u64 {
        self.next_sample_index
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    fn trim_to_capacity(&mut self) {
        while self.samples.len() > self.max_samples {
            self.samples.pop_front();
            self.base_sample_index += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_samples_returns_global_span() {
        let mut buffer = RollingAudioBuffer::new(10);

        let span = buffer.push_samples(&[1.0, 2.0, 3.0]);

        assert_eq!(
            span,
            AudioFrameSpan {
                start_sample: 0,
                end_sample: 3
            }
        );

        let span = buffer.push_samples(&[4.0, 5.0]);

        assert_eq!(
            span,
            AudioFrameSpan {
                start_sample: 3,
                end_sample: 5
            }
        );
    }

    #[test]
    fn slice_returns_expected_samples() {
        let mut buffer = RollingAudioBuffer::new(10);

        buffer.push_samples(&[0.0, 1.0, 2.0, 3.0, 4.0]);

        let slice = buffer.slice(1, 4).unwrap();

        assert_eq!(slice, vec![1.0, 2.0, 3.0]);
    }

    #[test]
    fn trims_old_samples_when_capacity_is_exceeded() {
        let mut buffer = RollingAudioBuffer::new(5);

        buffer.push_samples(&[0.0, 1.0, 2.0]);
        buffer.push_samples(&[3.0, 4.0, 5.0, 6.0]);

        assert_eq!(buffer.base_sample_index(), 2);
        assert_eq!(buffer.next_sample_index(), 7);
        assert_eq!(buffer.len(), 5);

        let slice = buffer.slice(2, 7).unwrap();

        assert_eq!(slice, vec![2.0, 3.0, 4.0, 5.0, 6.0]);
    }

    #[test]
    fn expired_slice_returns_error() {
        let mut buffer = RollingAudioBuffer::new(3);

        buffer.push_samples(&[0.0, 1.0, 2.0, 3.0]);

        let err = buffer.slice(0, 2).unwrap_err();

        assert_eq!(
            err,
            RollingBufferError::RangeExpired {
                requested_start: 0,
                available_start: 1
            }
        );
    }
}
