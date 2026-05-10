use std::time::{Duration, Instant};

use anyhow::Result;
use audio::source::AudioSource;
use core::AudioRuntime;
use tracing_subscriber::EnvFilter;

fn main() -> Result<()> {
    init_tracing();

    let mut runtime = AudioRuntime::new(AudioSource::Monitor)?;
    let transcript_rx = runtime.transcriptions();

    runtime.start()?;

    let started_at = Instant::now();
    let duration = Duration::from_secs(120);

    while started_at.elapsed() < duration {
        match transcript_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(chunk) => {
                if !chunk.body.is_empty() {
                    tracing::debug!(
                        index = chunk.index,
                        text_len = chunk.body.len(),
                        "transcript received by demo"
                    );
                    println!("{}", chunk.body);
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
        }
    }

    runtime.stop();

    Ok(())
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_writer(std::io::stderr)
        .try_init();
}
