use cpal::Sample;

pub(crate) fn interleaved_to_mono_f32<T>(input: &[T], channels: usize, output: &mut Vec<f32>)
where
    T: Sample + Copy,
    f32: cpal::FromSample<T>,
{
    if channels == 0 {
        return;
    }

    for frame in input.chunks_exact(channels) {
        let mut sum = 0.0f32;

        for sample in frame {
            sum += sample.to_sample::<f32>();
        }

        output.push(sum / channels as f32);
    }
}
