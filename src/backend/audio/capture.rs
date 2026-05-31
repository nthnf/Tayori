use super::device::AudioDevice;
use anyhow::Result;
use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample, Stream, StreamConfig};
use ringbuf::{HeapProd, traits::Producer};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tracing::{info, warn};

/// CPAL capture path.
///
/// `Capture` owns the CPAL stream handle, but CPAL owns the actual callback
/// execution loop. The callback closure is what receives samples and pushes
/// them into the raw ring buffer.
///
/// Flow:
/// CPAL stream -> raw ring.
///
/// Capture owns stream and shutdown flag. Pipeline owner provides raw ring
/// producer, and worker code above capture handles resample and VAD.
///
/// Capture starts stopped. `start()` clears stop flag, then starts CPAL stream.
pub struct Capture {
    /// Live CPAL input stream.
    stream: Stream,
    /// Shutdown flag for callback.
    pub stop: Arc<AtomicBool>,
}

impl Capture {
    /// Build capture pipeline for selected audio device.
    ///
    /// Initializes raw capture and CPAL stream, but does not start audio until
    /// `start()` is called.
    pub fn new(
        audio_device: AudioDevice,
        producer: HeapProd<f32>,
        stop: Arc<AtomicBool>,
    ) -> Result<Self> {
        let device = &audio_device.device;

        // Ask CPAL for the native input format. We convert every sample type
        // into f32 later so the rest of the pipeline has one audio format.
        let input_config = device.default_input_config()?;
        let stream_config: StreamConfig = input_config.clone().into();
        let sample_format = input_config.sample_format();
        let device_name = device
            .description()
            .map(|description| description.name().to_string())
            .unwrap_or_else(|_| "unknown".to_string());

        info!(
            device = %device_name,
            sample_rate = stream_config.sample_rate,
            channels = stream_config.channels,
            ?sample_format,
            "capture stream configured"
        );

        // CPAL stream callbacks are monomorphized by sample type. After this
        // match, all callbacks push normalized f32 samples into the same ring.
        let stream = match sample_format {
            SampleFormat::F32 => {
                build_input_stream::<f32>(device, &stream_config, producer, stop.clone())?
            }
            SampleFormat::I16 => {
                build_input_stream::<i16>(device, &stream_config, producer, stop.clone())?
            }
            SampleFormat::U16 => {
                build_input_stream::<u16>(device, &stream_config, producer, stop.clone())?
            }
            other => {
                return Err(anyhow::anyhow!("unsupported sample format: {other:?}"));
            }
        };

        Ok(Capture { stream, stop })
    }

    /// Start CPAL stream.
    pub fn start(&mut self) -> Result<()> {
        info!("capture stream starting");
        self.stream.play()?;
        info!("capture stream started");
        Ok(())
    }

    /// Stop CPAL stream.
    pub fn stop(&mut self) -> Result<()> {
        info!("capture stream stopping");
        self.stream.pause()?;
        info!("capture stream stopped");
        Ok(())
    }
}

fn build_input_stream<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    mut producer: HeapProd<f32>,
    stop: Arc<AtomicBool>,
) -> Result<Stream>
where
    T: Sample + SizedSample + Copy + Send + 'static,
    f32: FromSample<T>,
{
    // CPAL owns this callback thread. Keep it tiny: convert samples, push into
    // ring, count drops. Any heavy work here risks audio glitches.
    let stream = device.build_input_stream(
        *config,
        move |data: &[T], _| {
            push_input_samples(data, &mut producer, &stop);
        },
        move |err| {
            warn!(?err, "CPAL stream error");
        },
        None,
    )?;

    Ok(stream)
}

fn push_input_samples<T>(data: &[T], producer: &mut HeapProd<f32>, stop: &AtomicBool)
where
    T: Sample + Copy,
    f32: FromSample<T>,
{
    if stop.load(Ordering::Relaxed) {
        return;
    }

    let mut dropped = 0_u64;

    // Convert interleaved device samples into normalized f32 and write them to
    // the raw ring. Channel mixing happens in the resample worker, not here.
    for sample in data.iter().copied() {
        let sample = f32::from_sample(sample);

        if producer.try_push(sample).is_err() {
            dropped += 1;
        }
    }

    if dropped > 0 {
        warn!(dropped, "audio raw ring full; dropping capture samples");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ringbuf::traits::Consumer;
    use ringbuf::{HeapRb, traits::Split};

    #[test]
    fn push_input_samples_writes_converted_audio() {
        let rb = HeapRb::<f32>::new(8);
        let (mut producer, mut consumer) = rb.split();
        let stop = AtomicBool::new(false);

        push_input_samples(&[0.25_f32, 0.5_f32, 0.75_f32], &mut producer, &stop);

        assert_eq!(consumer.try_pop(), Some(0.25));
        assert_eq!(consumer.try_pop(), Some(0.5));
        assert_eq!(consumer.try_pop(), Some(0.75));
        assert_eq!(consumer.try_pop(), None);
    }

    #[test]
    fn push_input_samples_does_nothing_when_stopped() {
        let rb = HeapRb::<f32>::new(8);
        let (mut producer, mut consumer) = rb.split();
        let stop = AtomicBool::new(true);

        push_input_samples(&[0.25_f32, 0.5_f32], &mut producer, &stop);

        assert_eq!(consumer.try_pop(), None);
    }
}
