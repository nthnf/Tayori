use anyhow::Result;
use chrono::Utc;
use migration::MigratorTrait;
use ringbuf::{
    HeapRb,
    traits::{Consumer, Observer, Producer, Split},
};
use sea_orm::{ActiveModelTrait, Set};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};
use tayori::backend::audio::resample::{ResampleConfig, ResampleWorker};
use tayori::backend::audio::vad::{SpeechChunk, VadConfig, VadWorker};
use tayori::backend::detection::IntentDetector;
use tayori::backend::entities::{document_chunks, documents, projects};
use tayori::backend::models::install::moonshine_path;
use tayori::backend::models::moonshine::StreamingModel;
use tayori::backend::search::smart_hybrid_search;

// Simple deterministic pseudo-random helper to avoid external rand dependency
fn lcg_rand(seed: &mut u32) -> f32 {
    *seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
    (*seed as f32) / (u32::MAX as f32)
}

// Simple WAV file parser supporting 16-bit PCM, 32-bit PCM, and 32-bit Float formats
fn read_wav_file(path: &str) -> Option<(Vec<f32>, u32, usize)> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.len() < 44 {
        return None;
    }
    if &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }

    let mut offset = 12;
    let mut channels = 0u16;
    let mut sample_rate = 0u32;
    let mut bits_per_sample = 0u16;
    let mut format = 0u16;

    while offset + 8 <= bytes.len() {
        let chunk_id = &bytes[offset..offset + 4];
        let chunk_len = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().ok()?) as usize;
        offset += 8;

        if chunk_id == b"fmt " {
            if chunk_len < 16 || offset + chunk_len > bytes.len() {
                return None;
            }
            format = u16::from_le_bytes(bytes[offset..offset + 2].try_into().ok()?);
            channels = u16::from_le_bytes(bytes[offset + 2..offset + 4].try_into().ok()?);
            sample_rate = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().ok()?);
            bits_per_sample = u16::from_le_bytes(bytes[offset + 14..offset + 16].try_into().ok()?);
        } else if chunk_id == b"data" {
            if offset + chunk_len > bytes.len() {
                return None;
            }
            let data_bytes = &bytes[offset..offset + chunk_len];
            let mut samples = Vec::new();

            match (format, bits_per_sample) {
                (1, 16) => {
                    for chunk in data_bytes.chunks_exact(2) {
                        let val = i16::from_le_bytes(chunk.try_into().ok()?);
                        samples.push(val as f32 / 32768.0);
                    }
                }
                (1, 32) => {
                    for chunk in data_bytes.chunks_exact(4) {
                        let val = i32::from_le_bytes(chunk.try_into().ok()?);
                        samples.push(val as f32 / 2147483648.0);
                    }
                }
                (3, 32) => {
                    for chunk in data_bytes.chunks_exact(4) {
                        let val = f32::from_le_bytes(chunk.try_into().ok()?);
                        samples.push(val);
                    }
                }
                _ => return None,
            }
            return Some((samples, sample_rate, channels as usize));
        }
        offset += chunk_len;
    }
    None
}

// Memory reader on Linux (RSS)
fn get_rss_bytes() -> usize {
    std::fs::read_to_string("/proc/self/statm")
        .ok()
        .and_then(|content| {
            let pages = content.split_whitespace().nth(1)?.parse::<usize>().ok()?;
            Some(pages * 4096)
        })
        .unwrap_or(0)
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=================================================================");
    println!("                  TAYORI COMPONENT BENCHMARK                      ");
    println!("=================================================================\n");

    let initial_mem = get_rss_bytes();
    println!(
        "Base Process Memory (RSS): {:.2} MB\n",
        initial_mem as f64 / 1_048_576.0
    );

    // -----------------------------------------------------------------
    // 1. Benchmark Stage 1/2 Intent Detector
    // -----------------------------------------------------------------
    println!("--- 1. BENCHMARKING INTENT DETECTOR (TinyBERT ONNX) ---");
    let start_mem = get_rss_bytes();
    let init_start = Instant::now();

    // Load detector
    let detector = IntentDetector::new()?;
    let init_time = init_start.elapsed();
    let end_mem = get_rss_bytes();

    println!("✔ Initialization Latency: {:.2?}", init_time);
    println!(
        "✔ Memory Footprint Delta: {:.2} MB",
        (end_mem.saturating_sub(start_mem)) as f64 / 1_048_576.0
    );

    let test_cases = [
        "how do i fix this null pointer bug",
        "show me the sorting algorithm implementation",
        "yeah I think we can just leave it like that",
        "explain the database schema and foreign keys",
        "my screen is completely frozen right now",
    ];

    let start_inf = Instant::now();
    let iterations = 50;
    for i in 0..iterations {
        let text = test_cases[i % test_cases.len()];
        let _ = detector.detect(text)?;
    }
    let elapsed = start_inf.elapsed();
    let avg_latency = elapsed / iterations as u32;

    println!("✔ Average Inference Latency: {:.2?}", avg_latency);
    println!(
        "✔ Throughput: {:.2} inferences/sec\n",
        iterations as f64 / elapsed.as_secs_f64()
    );

    // Load Wav file or fallback
    let (mut audio_data, sample_rate, channels) = match read_wav_file("temp/test.wav") {
        Some((samples, rate, chs)) => {
            println!(
                "✔ Loaded real WAV file: temp/test.wav ({}Hz, {} channels, {:.2}s total)",
                rate,
                chs,
                samples.len() as f64 / (rate as f64 * chs as f64)
            );
            (samples, rate, chs)
        }
        None => {
            println!(
                "⚠ Could not load temp/test.wav (or format unsupported). Falling back to synthetic 10s Stereo audio."
            );
            let rate = 48000u32;
            let chs = 2usize;
            let sample_count = (rate as usize) * chs * 10;
            let mut mock_stereo_audio = vec![0.0f32; sample_count];
            for (i, x) in mock_stereo_audio.iter_mut().enumerate() {
                *x = ((i as f32) * 440.0 * 2.0 * std::f32::consts::PI / rate as f32).sin();
            }
            (mock_stereo_audio, rate, chs)
        }
    };

    // Limit to the first 60 seconds for benchmark speed. Set to None to process the full file.
    let limit_duration: Option<f64> = None;
    if let Some(limit) = limit_duration {
        let max_samples = (limit * sample_rate as f64 * channels as f64) as usize;
        if audio_data.len() > max_samples {
            println!(
                "✔ Limiting benchmark to the first {:.2}s of audio (edit `limit_duration` in benchmark.rs to None for full file)",
                limit
            );
            audio_data.truncate(max_samples);
        }
    }

    let audio_duration = audio_data.len() as f64 / (sample_rate as f64 * channels as f64);

    // -----------------------------------------------------------------
    // 2. Benchmark Streaming Pipeline (Resample + VAD + STT + Intent Detection + DB Search)
    // -----------------------------------------------------------------
    println!(
        "--- 2. BENCHMARKING STREAMING PIPELINE (Resampler -> VAD -> STT -> Intent Detector -> DB Search) ---"
    );
    let start_mem_pipeline = get_rss_bytes();

    // Initialize in-memory SQLite database (Safe: memory-only, does not touch real Tayori DB)
    let db = tayori::backend::db::connect("sqlite::memory:").await?;
    migration::Migrator::up(&db, None).await?;
    tayori::backend::db::init_vector_indexes(&db).await?;

    // Create a mock project
    let project_id = "bench-project".to_string();
    projects::ActiveModel {
        id: Set(project_id.clone()),
        name: Set("Bench Project".to_string()),
        created_at: Set(Utc::now()),
        updated_at: Set(Utc::now()),
        ..Default::default()
    }
    .insert(&db)
    .await?;

    // Insert mock interviewee persona documents matching the audio content
    let mock_docs = [
        (
            "relocation",
            "Keith Johnson Relocation Details",
            "Keith Johnson is relocating from northeastern Ohio to Jacksonville because his wife's job is moving down south and he is coming along with her.",
        ),
        (
            "passions",
            "Keith Passions and Experience",
            "Keith's main interests and passions are being outside, outdoor activities, and working with people. He has prior experience as a teacher helping people.",
        ),
        (
            "rei-position",
            "REI Customer Service Match",
            "The customer service position at REI Jacksonville aligns with Keith's background as a teacher and his passion for the outdoors.",
        ),
        (
            "relocation-guide",
            "Southern Region Relocation Guide",
            "Jacksonville relocation details. Southern regional offices are welcoming employees coming down south.",
        ),
        (
            "culture",
            "REI Company Culture and Management",
            "REI company culture. The work atmosphere is very welcoming and upper management treatment of employees helps retain employment.",
        ),
    ];

    let mut seed = 12345u32;
    for (i, (doc_name, source_name, content)) in mock_docs.iter().enumerate() {
        let doc_id = format!("doc-{}", doc_name);
        documents::ActiveModel {
            id: Set(doc_id.clone()),
            project_id: Set(project_id.clone()),
            source_name: Set(source_name.to_string()),
            status: Set("ready".to_string()),
            created_at: Set(Utc::now()),
            updated_at: Set(Utc::now()),
            ..Default::default()
        }
        .insert(&db)
        .await?;

        let mut mock_vector = vec![0.0f32; 384];
        for x in mock_vector.iter_mut() {
            *x = lcg_rand(&mut seed);
        }
        let vector_bytes: Vec<u8> = mock_vector.iter().flat_map(|f| f.to_le_bytes()).collect();

        document_chunks::ActiveModel {
            id: Set(format!("chunk-{}", i)),
            document_id: Set(doc_id),
            chunk_index: Set(0),
            content: Set(content.to_string()),
            vector: Set(vector_bytes),
            created_at: Set(Utc::now()),
        }
        .insert(&db)
        .await?;
    }

    // Load Moonshine STT Model
    let moonshine_model_path = moonshine_path("tiny", None);
    let mut stt = StreamingModel::load(
        &moonshine_model_path,
        0,
        &tayori::backend::models::moonshine::Quantization::default(),
    )?;
    let mut stt_state = stt.create_state();

    let config = ResampleConfig {
        input_sample_rate: sample_rate,
        output_sample_rate: 16000,
        channels,
    };

    let vad_config = VadConfig::default();

    // We use small 65536 sample ring buffers to simulate streaming and prevent O(N^2) memory shift bottlenecks
    let (mut raw_prod, raw_cons) = HeapRb::<f32>::new(65536).split();
    let (resampled_prod, resampled_cons) = HeapRb::<f32>::new(65536).split();
    let (vad_prod, mut vad_cons) = HeapRb::<SpeechChunk>::new(100).split();

    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_flag_vad = Arc::new(AtomicBool::new(false));

    let mut worker = ResampleWorker::new(config, raw_cons, resampled_prod, stop_flag.clone())?;
    let mut worker_vad =
        VadWorker::new(vad_config, resampled_cons, vad_prod, stop_flag_vad.clone())?;

    let end_mem_pipeline = get_rss_bytes();
    println!(
        "✔ Pipeline Memory Delta: {:.2} MB",
        (end_mem_pipeline.saturating_sub(start_mem_pipeline)) as f64 / 1_048_576.0
    );

    worker.start()?;
    worker_vad.start()?;

    let start_pipeline = Instant::now();
    let mut detected_speech_segments = 0;
    let mut pushed = 0;
    let chunk_size = 2048; // ~21ms at 48kHz stereo

    while pushed < audio_data.len() || raw_prod.occupied_len() > 0 {
        // 1. Try to push as much of the next chunk as possible
        if pushed < audio_data.len() {
            let end = (pushed + chunk_size).min(audio_data.len());
            let chunk = &audio_data[pushed..end];
            let written = raw_prod.push_slice(chunk);
            pushed += written;
        }

        // 2. Poll VAD events in real time
        while let Some(chunk) = vad_cons.try_pop() {
            detected_speech_segments += 1;

            // Feed chunk audio to Moonshine STT state
            if let Err(e) = stt.process_audio_chunk(&mut stt_state, &chunk.samples) {
                eprintln!("  [STT Error] Failed to process chunk audio: {:?}", e);
            }

            println!(
                "  [VAD Event] Speech Chunk #{}: Start={:.2}s, End={:.2}s, Duration={}ms (is_end={})",
                detected_speech_segments,
                chunk.start_ms as f64 / 1000.0,
                chunk.end_ms as f64 / 1000.0,
                chunk.end_ms.saturating_sub(chunk.start_ms),
                chunk.is_end_of_speech
            );

            if chunk.is_end_of_speech {
                match stt.decode_current_state(&mut stt_state, true) {
                    Ok(text) => {
                        let transcribed_text = text.trim().to_string();
                        println!("    [STT Transcript] \"{}\"", transcribed_text);

                        if !transcribed_text.is_empty() {
                            let det_start = Instant::now();
                            let intent = detector.detect(&transcribed_text)?;
                            let det_latency = det_start.elapsed();
                            println!(
                                "    [Intent Detector] Intent: {:?} (Latency: {:.2?})",
                                intent, det_latency
                            );

                            if intent.is_actionable {
                                // Trigger integrated SQLite hybrid search against mock persona docs
                                let mut query_vector = vec![0.0f32; 384];
                                for x in query_vector.iter_mut() {
                                    *x = lcg_rand(&mut seed);
                                }
                                let search_start = Instant::now();
                                let (_fts, _vec, results) = smart_hybrid_search(
                                    &db,
                                    &transcribed_text,
                                    query_vector,
                                    Some(2),
                                )
                                .await?;
                                let search_latency = search_start.elapsed();
                                println!(
                                    "      [Search Triggered] Found {} matching chunks in {:.2?}",
                                    results.len(),
                                    search_latency
                                );
                                for (idx, res) in results.iter().enumerate() {
                                    println!(
                                        "        Rank #{}: Score={:.4} | Content: \"{}\"",
                                        idx + 1,
                                        res.raw_score,
                                        res.content
                                    );
                                }
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("    [STT Error] Failed to decode segment text: {:?}", e);
                    }
                }
                stt.reset_state(&mut stt_state);
            }
        }

        // Sleep briefly to prevent high CPU utilization during waiting / yielding
        std::thread::sleep(Duration::from_micros(500));
    }

    // Give workers a brief moment to process final samples
    std::thread::sleep(Duration::from_millis(50));

    // Stop the resampler gracefully to trigger its flush
    stop_flag.store(true, std::sync::atomic::Ordering::Relaxed);
    worker.stop().unwrap();

    // Wait a brief moment for any flushed output from resampler to flow through VAD
    std::thread::sleep(Duration::from_millis(50));

    // Stop VAD gracefully to trigger its flush
    stop_flag_vad.store(true, std::sync::atomic::Ordering::Relaxed);
    worker_vad.stop().unwrap();

    // Drain all remaining/flushed VAD events
    while let Some(chunk) = vad_cons.try_pop() {
        detected_speech_segments += 1;

        if let Err(e) = stt.process_audio_chunk(&mut stt_state, &chunk.samples) {
            eprintln!("  [STT Error] Failed to process chunk audio: {:?}", e);
        }

        println!(
            "  [VAD Event] Speech Chunk #{}: Start={:.2}s, End={:.2}s, Duration={}ms (is_end={})",
            detected_speech_segments,
            chunk.start_ms as f64 / 1000.0,
            chunk.end_ms as f64 / 1000.0,
            chunk.end_ms.saturating_sub(chunk.start_ms),
            chunk.is_end_of_speech
        );

        if chunk.is_end_of_speech {
            match stt.decode_current_state(&mut stt_state, true) {
                Ok(text) => {
                    let transcribed_text = text.trim().to_string();
                    println!("    [STT Transcript] \"{}\"", transcribed_text);

                    if !transcribed_text.is_empty() {
                        let det_start = Instant::now();
                        let intent = detector.detect(&transcribed_text)?;
                        let det_latency = det_start.elapsed();
                        println!(
                            "    [Intent Detector] Intent: {:?} (Latency: {:.2?})",
                            intent, det_latency
                        );

                        if intent.is_actionable {
                            // Trigger integrated SQLite hybrid search against mock persona docs
                            let mut query_vector = vec![0.0f32; 384];
                            for x in query_vector.iter_mut() {
                                *x = lcg_rand(&mut seed);
                            }
                            let search_start = Instant::now();
                            let (_fts, _vec, results) =
                                smart_hybrid_search(&db, &transcribed_text, query_vector, Some(2))
                                    .await?;
                            let search_latency = search_start.elapsed();
                            println!(
                                "      [Search Triggered] Found {} matching chunks in {:.2?}",
                                results.len(),
                                search_latency
                            );
                            for (idx, res) in results.iter().enumerate() {
                                println!(
                                    "        Rank #{}: Score={:.4} | Content: \"{}\"",
                                    idx + 1,
                                    res.raw_score,
                                    res.content
                                );
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!("    [STT Error] Failed to decode segment text: {:?}", e);
                }
            }
            stt.reset_state(&mut stt_state);
        }
    }

    let pipeline_time = start_pipeline.elapsed();
    println!(
        "✔ Processed {:.2}s WAV Audio in: {:.2?}",
        audio_duration, pipeline_time
    );
    println!(
        "✔ Streaming Pipeline Real-time Factor (RTF): {:.2}x\n",
        audio_duration / pipeline_time.as_secs_f64()
    );

    // -----------------------------------------------------------------
    // 4. Benchmark SQLite Vector & FTS5 Hybrid Search
    // -----------------------------------------------------------------
    println!("--- 4. BENCHMARKING HYBRID SEARCH (SQLite In-Memory) ---");
    let mut query_vector = vec![0.0f32; 384];
    for x in query_vector.iter_mut() {
        *x = lcg_rand(&mut seed);
    }

    let search_start = Instant::now();
    let search_iterations = 100;
    for _ in 0..search_iterations {
        let _ = smart_hybrid_search(
            &db,
            "Jacksonville relocation details",
            query_vector.clone(),
            Some(5),
        )
        .await?;
    }
    let search_elapsed = search_start.elapsed();
    let avg_search_latency = search_elapsed / search_iterations;

    println!(
        "✔ Average Hybrid Search Latency (Seeded Database): {:.2?}",
        avg_search_latency
    );
    println!(
        "✔ Search Throughput: {:.2} queries/sec\n",
        search_iterations as f64 / search_elapsed.as_secs_f64()
    );

    println!("=================================================================");
    println!("                  BENCHMARKS COMPLETED SUCCESSFULLY              ");
    println!("=================================================================");

    Ok(())
}
