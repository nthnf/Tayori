use anyhow::Result;
use ndarray::{Array1, Array2, ArrayD, s};
use ort::{execution_providers::CPUExecutionProvider, session::Session, value::Tensor};
use std::path::Path;

#[derive(Debug, Clone)]
pub enum VadEvent {
    Start(u64),
    End(u64),
}

pub struct SileroVad {
    session: Session,
    state: ArrayD<f32>,
    context: Array2<f32>,
    threshold: f32,
    sample_rate: u32,

    // Hysteresis states
    triggered: bool,
    temp_end_sample: u64,
    current_sample: u64,

    // Settings
    min_silence_samples: u64,
    speech_pad_samples: u64,
}

impl SileroVad {
    pub fn new(
        model_path: &Path,
        threshold: f32,
        sample_rate: u32,
        min_silence_ms: u32,
        speech_pad_ms: u32,
    ) -> Result<Self> {
        let session = Session::builder()
            .map_err(|e| anyhow::anyhow!(e.to_string()))?
            .with_intra_threads(1)
            .map_err(|e| anyhow::anyhow!(e.to_string()))?
            .with_inter_threads(1)
            .map_err(|e| anyhow::anyhow!(e.to_string()))?
            .with_execution_providers([CPUExecutionProvider::default().build()])
            .map_err(|e| anyhow::anyhow!(e.to_string()))?
            .commit_from_file(model_path)
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;

        let state = ArrayD::<f32>::zeros(ndarray::IxDyn(&[2, 1, 128]));
        let context = Array2::<f32>::zeros((1, 64));

        let min_silence_samples = (sample_rate as u64 * min_silence_ms as u64) / 1000;
        let speech_pad_samples = (sample_rate as u64 * speech_pad_ms as u64) / 1000;

        Ok(Self {
            session,
            state,
            context,
            threshold,
            sample_rate,
            triggered: false,
            temp_end_sample: 0,
            current_sample: 0,
            min_silence_samples,
            speech_pad_samples,
        })
    }

    pub fn reset_states(&mut self) {
        self.state.fill(0.0);
        self.context.fill(0.0);
        self.triggered = false;
        self.temp_end_sample = 0;
        self.current_sample = 0;
    }

    pub fn process_chunk(&mut self, chunk: &[f32]) -> Result<Option<VadEvent>> {
        let batch_size = 1;
        let chunk_size = chunk.len();
        let context_size = 64; // fixed for 16kHz

        let input_array = Array2::from_shape_vec((1, chunk_size), chunk.to_vec())?;

        let mut concatenated = Array2::<f32>::zeros((batch_size, context_size + chunk_size));
        concatenated
            .slice_mut(s![.., 0..context_size])
            .assign(&self.context);
        concatenated
            .slice_mut(s![.., context_size..])
            .assign(&input_array);

        let input_tensor = Tensor::from_array(concatenated)?;
        let state_tensor = Tensor::from_array(self.state.clone())?;
        let sr_tensor = Tensor::from_array(Array1::<i64>::from_elem(1, self.sample_rate as i64))?;

        let inputs = ort::inputs![input_tensor, state_tensor, sr_tensor];
        let outputs = self.session.run(inputs)?;

        // Extract State
        let state_key = if outputs.contains_key("stateN") {
            "stateN"
        } else {
            "state"
        };
        let (state_shape, state_data) = outputs[state_key].try_extract_tensor::<f32>()?;
        self.state = ArrayD::<f32>::from_shape_vec(state_shape.to_ixdyn(), state_data.to_vec())?;

        // Extract Output
        let output_key = if outputs.contains_key("output") {
            "output"
        } else {
            outputs
                .iter()
                .next()
                .ok_or_else(|| anyhow::anyhow!("Silero VAD returned no outputs"))?
                .0
        };
        let (_, output_data) = outputs[output_key].try_extract_tensor::<f32>()?;

        let speech_prob = output_data[0];

        // Update context
        self.context = input_array
            .slice(s![.., (chunk_size - context_size)..])
            .to_owned();

        self.current_sample += chunk_size as u64;

        // VAD Hysteresis logic
        let mut event = None;

        if speech_prob >= self.threshold {
            self.temp_end_sample = 0;
            if !self.triggered {
                self.triggered = true;
                // Emit start
                let start_ts = self
                    .current_sample
                    .saturating_sub(chunk_size as u64)
                    .saturating_sub(self.speech_pad_samples);
                event = Some(VadEvent::Start(start_ts));
            }
        } else {
            if self.triggered {
                if self.temp_end_sample == 0 {
                    self.temp_end_sample = self.current_sample;
                } else if self.current_sample - self.temp_end_sample >= self.min_silence_samples {
                    self.triggered = false;
                    let end_ts = self.temp_end_sample + self.speech_pad_samples;
                    event = Some(VadEvent::End(end_ts));
                    self.temp_end_sample = 0;
                }
            }
        }

        Ok(event)
    }
}
