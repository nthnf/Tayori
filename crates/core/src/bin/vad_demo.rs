use std::time::{Duration, Instant};

use anyhow::Result;
use audio::{CaptureDevice, CpalCapture, CpalCaptureConfig, SileroVadConfig, SileroVadSegmenter};
use tracing::{info, warn};

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_target(false)
        .with_level(true)
        .init();

    info!("Tayori VAD demo starting");

    let capture = CpalCapture::new(CpalCaptureConfig {
        device: CaptureDevice::DefaultOutput,
        target_sample_rate: 16_000,
        ring_seconds: 5,

        // 32ms at 16kHz = 512 samples.
        // This matches Silero streaming chunks.
        output_frame_ms: 32,

        frame_channel_capacity: 128,
    });

    let handle = capture.start()?;

    let mut vad = SileroVadSegmenter::new(SileroVadConfig {
        threshold: 0.55,
        min_segment_ms: 1_000,
        max_segment_ms: 8_000,
        pre_roll_frames: 3,
        ..Default::default()
    })?;

    let started_at = Instant::now();
    let mut frames_seen = 0_u64;
    let mut segments_seen = 0_u64;

    info!("capturing and running VAD for 20 seconds");

    while started_at.elapsed() < Duration::from_secs(20) {
        match handle.frames.recv_timeout(Duration::from_millis(500)) {
            Ok(frame) => {
                frames_seen += 1;

                if frames_seen.is_multiple_of(31) {
                    info!(
                        frames_seen,
                        samples = frame.samples.len(),
                        duration_ms = frame.duration_ms(),
                        "received audio frame"
                    );
                }

                if let Some(segment) = vad.push_frame(frame)? {
                    segments_seen += 1;

                    info!(
                        segments_seen,
                        samples = segment.samples.len(),
                        duration_seconds = segment.duration_seconds(),
                        "VAD emitted speech segment"
                    );

                    // Next milestone:
                    // whisper_tx.send(segment)?;
                }
            }

            Err(err) => {
                warn!(?err, "no audio frame received");
            }
        }
    }

    if let Some(segment) = vad.flush() {
        segments_seen += 1;

        info!(
            segments_seen,
            samples = segment.samples.len(),
            duration_seconds = segment.duration_seconds(),
            "VAD flushed final speech segment"
        );
    }

    handle.stop()?;

    info!("Tayori VAD demo finished");

    Ok(())
}
