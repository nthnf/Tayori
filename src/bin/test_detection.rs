use anyhow::Result;
use tayori::backend::detection::IntentDetector;
use tokio::time::{Duration, sleep};

#[tokio::main]
async fn main() -> Result<()> {
    println!("Loading IntentDetector...");
    let detector = IntentDetector::new()?;

    // 1. Actionable tests (explicitly actionable prompts)
    let actionable_stream = vec![
        "i would like to know your background",
        "it would be great if you could introduce yourself",
        "go ahead and introduce yourself",
        "just state your strengths and weaknesses",
        "i am curious about your previous experience",
        "how do i fix this null pointer bug",
        "show me the sorting algorithm implementation",
        "where is the config file for the dev server",
        "explain the database schema and foreign keys",
        "tell me about a time you solved a difficult technical challenge",
    ];

    println!("\n=== RUNNING ACTIONABLE TESTS (EXPECTED: PASS) ===\n");
    for sentence in actionable_stream {
        let result = detector.detect(sentence)?;
        if result.is_actionable {
            println!("✅ PASS: \"{}\" -> Prob: {:.4}", sentence, result.score);
        } else {
            println!(
                "❌ FAIL: \"{}\" -> Prob: {:.4} (Expected: Actionable)",
                sentence, result.score
            );
        }
        sleep(Duration::from_millis(10)).await;
    }

    // 2. Non-actionable / Noise tests
    let noise_stream = vec![
        "my code is crashing on startup",
        "the database connection keeps timing out",
        "we need to figure out this memory leak",
        "yeah I think we can just leave it like that",
        "sounds good to me",
        "what if I told you the earth was flat",
        "my screen is completely frozen right now",
        "I have a hard stop in five minutes",
        "I think Bob is muted, can you hear us",
        "wow that is actually really crazy",
        "so if you look here the compiler is throwing an error",
        "can you hear me",
        "is my voice clear",
        "am i audible",
        "can everyone see my screen",
    ];

    println!("\n=== RUNNING NOISE / NEGATIVE TESTS (EXPECTED: IGNORED) ===\n");
    for sentence in noise_stream {
        let result = detector.detect(sentence)?;
        if !result.is_actionable {
            println!(
                "✅ PASS: \"{}\" -> Prob: {:.4} (Ignored)",
                sentence, result.score
            );
        } else {
            println!(
                "❌ FAIL: \"{}\" -> Prob: {:.4} (Expected: Ignored)",
                sentence, result.score
            );
        }
        sleep(Duration::from_millis(10)).await;
    }

    Ok(())
}
