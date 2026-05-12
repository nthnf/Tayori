use crate::device::AudioDevice;
use anyhow::Result;
use cpal::{
    Device, FromSample, Sample, SampleFormat, SizedSample, Stream, StreamConfig,
    traits::{DeviceTrait, StreamTrait},
};
use ringbuf::HeapProd;
use ringbuf::traits::Producer;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

/// CPAL capture path.
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
    pub stream: Stream,
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
    device: &Device,
    config: &StreamConfig,
    mut producer: HeapProd<f32>,
    stop: Arc<AtomicBool>,
) -> Result<Stream>
where
    T: Sample + SizedSample + Copy + Send + 'static,
    f32: FromSample<T>,
{
    let mut stats = CaptureStats::new(config.sample_rate, config.channels as usize);

    // CPAL owns this callback thread. Keep it tiny: convert samples, push into
    // ring, count drops. Any heavy work here risks audio glitches.
    let stream = device.build_input_stream(
        *config,
        move |data: &[T], _| {
            push_input_samples(data, &mut producer, &stop, &mut stats);
        },
        move |err| {
            warn!(?err, "CPAL stream error");
        },
        None,
    )?;

    Ok(stream)
}

fn push_input_samples<T>(
    data: &[T],
    producer: &mut HeapProd<f32>,
    stop: &AtomicBool,
    stats: &mut CaptureStats,
) where
    T: Sample + Copy,
    f32: FromSample<T>,
{
    if stop.load(Ordering::Relaxed) {
        return;
    }

    let started_at = Instant::now();
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

    stats.record(data.len() as u64, dropped, started_at.elapsed());
}

struct CaptureStats {
    sample_rate: u32,
    channels: usize,
    last_report: Instant,
    callbacks: u64,
    samples: u64,
    dropped: u64,
    max_callback: Duration,
}

impl CaptureStats {
    fn new(sample_rate: u32, channels: usize) -> Self {
        Self {
            sample_rate,
            channels,
            last_report: Instant::now(),
            callbacks: 0,
            samples: 0,
            dropped: 0,
            max_callback: Duration::ZERO,
        }
    }

    fn record(&mut self, samples: u64, dropped: u64, callback_duration: Duration) {
        // Accumulate callback health counters and emit one compact report per
        // second. This keeps debug logs useful without flooding output.
        self.callbacks += 1;
        self.samples += samples;
        self.dropped += dropped;
        self.max_callback = self.max_callback.max(callback_duration);

        if self.last_report.elapsed() < Duration::from_secs(1) {
            return;
        }

        let audio_ms = if self.sample_rate == 0 || self.channels == 0 {
            0.0
        } else {
            self.samples as f64 / self.channels as f64 / self.sample_rate as f64 * 1_000.0
        };

        debug!(
            callbacks = self.callbacks,
            samples = self.samples,
            dropped = self.dropped,
            audio_ms,
            max_callback_ms = self.max_callback.as_secs_f64() * 1_000.0,
            "capture metrics"
        );

        self.last_report = Instant::now();
        self.callbacks = 0;
        self.samples = 0;
        self.dropped = 0;
        self.max_callback = Duration::ZERO;
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

        let mut stats = CaptureStats::new(16_000, 1);
        push_input_samples(
            &[0.25_f32, 0.5_f32, 0.75_f32],
            &mut producer,
            &stop,
            &mut stats,
        );

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

        let mut stats = CaptureStats::new(16_000, 1);
        push_input_samples(&[0.25_f32, 0.5_f32], &mut producer, &stop, &mut stats);

        assert_eq!(consumer.try_pop(), None);
    }
}
