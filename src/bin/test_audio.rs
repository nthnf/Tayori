use anyhow::Result;
use tayori::backend::audio::runtime::AudioRuntime;
use tayori::backend::audio::source::AudioSource;
use tayori::backend::models::install;
use tokio::signal;

#[tokio::main]
async fn main() -> Result<()> {
    println!("Initializing Tayori Audio Test...");

    // Default to tiny model for the test
    let model_type = "medium";

    let moonshine_path = install::moonshine_path(model_type, None);
    let silero_path = install::default_silero_path(None);

    if !moonshine_path.exists() || !silero_path.exists() {
        anyhow::bail!(
            "Required models are missing. Please run the main app once to download them."
        );
    }

    let moonshine_str = moonshine_path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("Invalid model path"))?;

    println!("Starting AudioRuntime (Monitoring system audio)...");
    let mut runtime = AudioRuntime::new(AudioSource::Monitor, moonshine_str)?;

    // Subscribe to both Draft and Stable updates
    let mut ui_rx = runtime.subscribe_ui();
    let mut segment_rx = runtime.subscribe_segment();

    runtime.start()?;

    println!("=======================================================");
    println!("Listening... Speak or play audio on your system!");
    println!("Press Ctrl+C to exit.");
    println!("=======================================================");

    loop {
        tokio::select! {
            // Handle Draft UI events
            Ok(ui_chunk) = ui_rx.recv() => {
                println!("[DRAFT] {}", ui_chunk.draft_text);
            }

            // Handle Finalized Segment events
            Ok(stable_segment) = segment_rx.recv() => {
                println!(">>> [FINAL] {}", stable_segment.full_text);
                println!("-------------------------------------------------------");
            }

            // Handle Ctrl-C
            _ = signal::ctrl_c() => {
                println!("\nCtrl-C received, shutting down audio pipeline...");
                break;
            }
        }
    }

    runtime.stop()?;
    println!("Done.");

    Ok(())
}
