#[derive(Debug, Clone)]
pub struct SpeechSegment {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

impl SpeechSegment {
    pub fn new(samples: Vec<f32>, sample_rate: u32) -> Self {
        Self {
            samples,
            sample_rate,
        }
    }

    pub fn duration_ms(&self) -> f32 {
        if self.sample_rate == 0 {
            return 0.0;
        }

        self.samples.len() as f32 / self.sample_rate as f32 * 1000.0
    }

    pub fn duration_seconds(&self) -> f32 {
        self.duration_ms() / 1000.0
    }
}
