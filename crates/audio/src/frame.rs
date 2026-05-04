#[derive(Debug, Clone)]
pub struct AudioFrame {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub channels: u16,
}

impl AudioFrame {
    pub fn mono_16k(samples: Vec<f32>) -> Self {
        Self {
            samples,
            sample_rate: 16_000,
            channels: 1,
        }
    }

    pub fn duration_ms(&self) -> f32 {
        if self.sample_rate == 0 || self.channels == 0 {
            return 0.0;
        }

        let frames = self.samples.len() as f32 / self.channels as f32;
        frames / self.sample_rate as f32 * 1000.0
    }
}
