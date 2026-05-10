use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use audio::{
    capture::Capture,
    device::AudioDevice,
    resample::{ResampleConfig, ResampleWorker},
    source::AudioSource,
};
use ringbuf::{
    HeapRb,
    traits::{Consumer, Split},
};

fn main() -> Result<()> {
    let output_path = debug_wav_path("capture-demo.wav")?;

    let audio_device = AudioDevice::from(AudioSource::Monitor)?;
    let input_config = audio_device.default_input_config()?;
    let input_sample_rate = input_config.sample_rate();
    let channels = input_config.channels() as usize;

    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (raw_producer, raw_consumer) = ring_for(input_sample_rate, channels)?;
    let (resampled_producer, mut resampled_consumer) = ring_for(16_000, 1)?;

    let mut capture = Capture::new(audio_device, raw_producer, stop.clone())?;
    let mut resample = ResampleWorker::new(
        ResampleConfig {
            input_sample_rate,
            output_sample_rate: 16_000,
            channels,
            batch_samples: 1024,
        },
        raw_consumer,
        resampled_producer,
        stop.clone(),
    )?;

    capture.start()?;
    resample.start()?;

    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    let mut writer = hound::WavWriter::create(&output_path, spec)
        .with_context(|| format!("failed to create WAV file: {}", output_path.display()))?;

    let duration = Duration::from_secs(15);
    let started_at = Instant::now();

    while started_at.elapsed() < duration {
        match resampled_consumer.try_pop() {
            Some(sample) => {
                writer.write_sample(f32_to_i16(sample))?;
            }
            None => {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }

    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    let _ = capture.stop();
    resample.stop();
    writer.finalize().context("failed to finalize WAV file")?;

    println!("{}", output_path.display());

    Ok(())
}

fn f32_to_i16(sample: f32) -> i16 {
    let sample = sample.clamp(-1.0, 1.0);
    (sample * i16::MAX as f32) as i16
}

fn ring_for(
    sample_rate: u32,
    channels: usize,
) -> Result<(ringbuf::HeapProd<f32>, ringbuf::HeapCons<f32>)> {
    anyhow::ensure!(sample_rate > 0, "sample_rate must be > 0");
    anyhow::ensure!(channels > 0, "channels must be > 0");

    let capacity = sample_rate as usize * channels * 10;
    let ring = HeapRb::<f32>::try_new(capacity)?;
    Ok(ring.split())
}

fn debug_wav_path(filename: &str) -> Result<PathBuf> {
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var_os("HOME").expect("HOME is not set");
            PathBuf::from(home).join(".local/share")
        });

    let dir = data_home.join("tayori").join("debug");

    fs::create_dir_all(&dir).with_context(|| format!("failed to create dir: {}", dir.display()))?;

    Ok(dir.join(filename))
}
