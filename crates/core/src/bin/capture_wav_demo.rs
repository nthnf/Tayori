use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use audio::{CaptureDevice, CpalCapture, CpalCaptureConfig, list_audio_devices};
use tracing::{info, warn};

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_target(false)
        .with_level(true)
        .init();

    info!("Tayori capture WAV demo starting");

    for device in list_audio_devices()? {
        info!(
            name = %device.name,
            default_input = device.is_default_input,
            default_output = device.is_default_output,
            has_input_config = device.has_default_input_config,
            has_output_config = device.has_default_output_config,
            looks_like_monitor = device.looks_like_monitor,
            "audio device"
        );
    }
    let output_path = debug_wav_path("capture-demo.wav")?;

    info!(path = %output_path.display(), "saving captured audio to WAV");

    let capture = CpalCapture::new(CpalCaptureConfig {
        // Try system/output audio first.
        device: CaptureDevice::DefaultMonitor,

        target_sample_rate: 16_000,
        ring_seconds: 5,
        output_frame_ms: 32,
        frame_channel_capacity: 128,
    });

    let handle = capture.start()?;

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

    let mut frames_seen = 0_u64;
    let mut samples_written = 0_u64;

    info!(
        seconds = duration.as_secs(),
        "recording now; play meeting/system audio"
    );

    while started_at.elapsed() < duration {
        match handle.frames.recv_timeout(Duration::from_millis(500)) {
            Ok(frame) => {
                frames_seen += 1;

                for sample in frame.samples {
                    writer.write_sample(f32_to_i16(sample))?;
                    samples_written += 1;
                }

                if frames_seen % 31 == 0 {
                    info!(frames_seen, samples_written, "capturing...");
                }
            }

            Err(err) => {
                warn!(?err, "no frame received");
            }
        }
    }

    handle.stop()?;

    writer.finalize().context("failed to finalize WAV file")?;

    let seconds_written = samples_written as f32 / 16_000.0;

    info!(
        path = %output_path.display(),
        samples_written,
        seconds_written,
        "WAV saved"
    );

    println!("{}", output_path.display());

    Ok(())
}

fn f32_to_i16(sample: f32) -> i16 {
    let sample = sample.clamp(-1.0, 1.0);
    (sample * i16::MAX as f32) as i16
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
