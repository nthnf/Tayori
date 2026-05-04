use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use audio::{CaptureDevice, CpalCapture, CpalCaptureConfig, list_audio_devices};

const SAMPLE_RATE: u32 = 16_000;
const DEFAULT_SECONDS: u64 = 20;

fn main() -> Result<()> {
    let args = Args::parse()?;

    if args.list_devices {
        print_devices()?;
        return Ok(());
    }

    let output_path = match args.output_path {
        Some(path) => path,
        None => debug_wav_path(&timestamped_filename())?,
    };

    println!("recording processed Tayori audio");
    println!("duration: {}s", args.seconds);
    println!("output:   {}", output_path.display());
    println!("format:   mono 16kHz 16-bit WAV");
    println!();

    let capture = CpalCapture::new(CpalCaptureConfig {
        device: CaptureDevice::DefaultMonitor,
        target_sample_rate: SAMPLE_RATE,
        ring_seconds: 10,
        output_frame_ms: 32,
        frame_channel_capacity: 512,
    });

    let handle = capture.start()?;

    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    ensure_parent_dir(&output_path)?;

    let mut writer = hound::WavWriter::create(&output_path, spec)
        .with_context(|| format!("failed to create WAV file: {}", output_path.display()))?;

    let duration = Duration::from_secs(args.seconds);
    let started_at = Instant::now();

    let mut stats = AudioStats::default();

    while started_at.elapsed() < duration {
        match handle.frames.recv_timeout(Duration::from_millis(500)) {
            Ok(frame) => {
                for sample in frame.samples {
                    stats.push(sample);
                    writer.write_sample(f32_to_i16(sample))?;
                }

                print_progress(started_at.elapsed(), duration, &stats);
            }

            Err(_) => {
                eprintln!("warning: no audio frame received for 500ms");
            }
        }
    }

    handle.stop()?;

    writer.finalize().context("failed to finalize WAV file")?;

    println!();
    println!();
    println!("saved: {}", output_path.display());
    println!("samples: {}", stats.samples_seen);
    println!("seconds: {:.2}", stats.duration_seconds());
    println!("peak:    {:.4}", stats.peak);
    println!("rms:     {:.4}", stats.rms());
    println!("clipped: {}", stats.clipped_samples);

    if stats.peak < 0.01 {
        println!();
        println!("warning: audio is extremely quiet or silent");
    }

    if stats.clipped_samples > 0 {
        println!();
        println!("warning: clipping detected; audio may sound distorted");
    }

    println!();
    println!("play with:");
    println!("  pw-play {}", output_path.display());

    Ok(())
}

#[derive(Debug)]
struct Args {
    seconds: u64,
    output_path: Option<PathBuf>,
    list_devices: bool,
}

impl Args {
    fn parse() -> Result<Self> {
        let mut seconds = DEFAULT_SECONDS;
        let mut output_path = None;
        let mut list_devices = false;

        let mut args = std::env::args().skip(1);

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--seconds" | "-s" => {
                    let value = args.next().context("--seconds requires a value")?;

                    seconds = value
                        .parse::<u64>()
                        .with_context(|| format!("invalid --seconds value: {value}"))?;
                }

                "--out" | "-o" => {
                    let value = args.next().context("--out requires a path")?;
                    output_path = Some(PathBuf::from(value));
                }

                "--list-devices" => {
                    list_devices = true;
                }

                "--help" | "-h" => {
                    print_help();
                    std::process::exit(0);
                }

                unknown => {
                    anyhow::bail!("unknown arg: {unknown}");
                }
            }
        }

        Ok(Self {
            seconds,
            output_path,
            list_devices,
        })
    }
}

fn print_help() {
    println!(
        r#"Usage:
  cargo run -p core --bin capture_wav_demo
  cargo run -p core --bin capture_wav_demo -- --seconds 30
  cargo run -p core --bin capture_wav_demo -- --out /tmp/tayori-test.wav
  cargo run -p core --bin capture_wav_demo -- --list-devices

Output:
  mono 16kHz 16-bit WAV, matching Tayori's VAD/STT input.
"#
    );
}

fn print_devices() -> Result<()> {
    for device in list_audio_devices()? {
        println!(
            "name={:?} default_input={} default_output={} input_config={} output_config={} monitor_like={}",
            device.name,
            device.is_default_input,
            device.is_default_output,
            device.has_default_input_config,
            device.has_default_output_config,
            device.looks_like_monitor,
        );
    }

    Ok(())
}

#[derive(Debug, Default)]
struct AudioStats {
    samples_seen: u64,
    sum_squares: f64,
    peak: f32,
    clipped_samples: u64,
}

impl AudioStats {
    fn push(&mut self, sample: f32) {
        self.samples_seen += 1;

        let abs = sample.abs();
        self.peak = self.peak.max(abs);

        self.sum_squares += sample as f64 * sample as f64;

        if abs >= 0.999 {
            self.clipped_samples += 1;
        }
    }

    fn rms(&self) -> f32 {
        if self.samples_seen == 0 {
            return 0.0;
        }

        (self.sum_squares / self.samples_seen as f64).sqrt() as f32
    }

    fn duration_seconds(&self) -> f32 {
        self.samples_seen as f32 / SAMPLE_RATE as f32
    }
}

fn print_progress(elapsed: Duration, total: Duration, stats: &AudioStats) {
    let elapsed_s = elapsed.as_secs_f32();
    let total_s = total.as_secs_f32();
    let percent = (elapsed_s / total_s * 100.0).clamp(0.0, 100.0);

    eprint!(
        "\rrecording {:>5.1}/{:.1}s {:>5.1}% | peak {:.3} | rms {:.3}",
        elapsed_s,
        total_s,
        percent,
        stats.peak,
        stats.rms(),
    );
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

fn timestamped_filename() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    format!("capture-processed-16k-{timestamp}.wav")
}

fn ensure_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create dir: {}", parent.display()))?;
    }

    Ok(())
}
