use anyhow::{Context, Result};
use rubato::{
    Async, FixedAsync, Resampler, SincInterpolationParameters, SincInterpolationType,
    WindowFunction, audioadapter_buffers::direct::SequentialSliceOfVecs, calculate_cutoff,
};

pub(crate) struct RubatoResampler {
    bypass: bool,

    resampler: Option<Async<f32>>,
    pending_input: Vec<f32>,

    input_buffer: Vec<Vec<f32>>,
    output_buffer: Vec<Vec<f32>>,
}

impl RubatoResampler {
    pub fn new(input_sample_rate: u32, output_sample_rate: u32) -> Result<Self> {
        if input_sample_rate == output_sample_rate {
            return Ok(Self {
                bypass: true,
                resampler: None,
                pending_input: Vec::new(),
                input_buffer: vec![Vec::new()],
                output_buffer: vec![Vec::new()],
            });
        }

        let sinc_len = 256;
        let window = WindowFunction::BlackmanHarris2;

        let params = SincInterpolationParameters {
            sinc_len,
            f_cutoff: calculate_cutoff(sinc_len, window),
            interpolation: SincInterpolationType::Cubic,
            oversampling_factor: 256,
            window,
        };

        let ratio = output_sample_rate as f64 / input_sample_rate as f64;

        let chunk_size = 1024;
        let channels = 1;

        let resampler = Async::<f32>::new_sinc(
            ratio,
            1.05,
            &params,
            chunk_size,
            channels,
            FixedAsync::Input,
        )
        .context("failed to create Rubato resampler")?;

        let input_max = resampler.input_frames_next();
        let output_max = resampler.output_frames_max();

        Ok(Self {
            bypass: false,
            resampler: Some(resampler),
            pending_input: Vec::with_capacity(input_max * 2),
            input_buffer: vec![vec![0.0; input_max]],
            output_buffer: vec![vec![0.0; output_max]],
        })
    }

    pub fn push(&mut self, input: &[f32], output: &mut Vec<f32>) -> Result<()> {
        if input.is_empty() {
            return Ok(());
        }

        if self.bypass {
            output.extend_from_slice(input);
            return Ok(());
        }

        let resampler = self
            .resampler
            .as_mut()
            .expect("resampler missing while bypass is false");

        self.pending_input.extend_from_slice(input);

        loop {
            let needed_input = resampler.input_frames_next();

            if self.pending_input.len() < needed_input {
                break;
            }

            if self.input_buffer[0].len() != needed_input {
                self.input_buffer[0].resize(needed_input, 0.0);
            }

            for sample in self.input_buffer[0].iter_mut().take(needed_input) {
                *sample = self.pending_input.remove(0);
            }

            let expected_output = resampler.output_frames_next();

            if self.output_buffer[0].len() != expected_output {
                self.output_buffer[0].resize(expected_output, 0.0);
            }

            let input_adapter = SequentialSliceOfVecs::new(&self.input_buffer, 1, needed_input)
                .context("failed to create Rubato input adapter")?;

            let mut output_adapter =
                SequentialSliceOfVecs::new_mut(&mut self.output_buffer, 1, expected_output)
                    .context("failed to create Rubato output adapter")?;

            let (_frames_in, frames_out) = resampler
                .process_into_buffer(&input_adapter, &mut output_adapter, None)
                .context("Rubato resampling failed")?;

            output.extend_from_slice(&self.output_buffer[0][..frames_out]);
        }

        Ok(())
    }
}
