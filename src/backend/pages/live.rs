use crate::backend::audio::runtime::AudioRuntime;
use crate::backend::audio::source::AudioSource;
use crate::backend::audio::transcript::{StableSegment, UiChunk};
use crate::backend::detection::IntentDetector;
use crate::backend::entities::{
    session_answers, sessions, settings, transcript_chunks, transcript_summaries,
};
use crate::backend::models::{install, llm::LlmModel};
use crate::backend::search::smart_hybrid_search;
use crate::state::AppState;
use anyhow::{Context, Result, anyhow};
use chrono::Utc;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, QueryOrder, Set};
use tokio::spawn;
use tokio::sync::broadcast;
use tracing::error;
pub struct LivePageData {
    pub is_ended: bool,
    pub transcripts: Vec<crate::backend::entities::transcript_chunks::Model>,
    pub qas: Vec<crate::backend::entities::session_answers::Model>,
}

pub struct LivePageModel {
    pub state: AppState,
}

impl LivePageModel {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    /// Fetches initial data needed to render the live transcription page.
    pub async fn load(&self, session_id: &str) -> Result<LivePageData> {
        let db = &self.state.db;

        let session = sessions::Entity::find_by_id(session_id.to_string())
            .one(db)
            .await
            .map_err(|e| anyhow!("Failed to fetch session: {}", e))?;

        let is_ended = session.map(|s| s.status == "completed").unwrap_or(false);

        let transcripts = transcript_chunks::Entity::find()
            .filter(transcript_chunks::Column::SessionId.eq(session_id))
            .order_by_asc(transcript_chunks::Column::ChunkIndex)
            .all(db)
            .await
            .map_err(|e| anyhow!("Failed to fetch transcripts: {}", e))?;

        let qas = session_answers::Entity::find()
            .filter(session_answers::Column::SessionId.eq(session_id))
            .order_by_asc(session_answers::Column::CreatedAt)
            .all(db)
            .await
            .map_err(|e| anyhow!("Failed to fetch qa history: {}", e))?;

        Ok(LivePageData {
            is_ended,
            transcripts,
            qas,
        })
    }

    /// Starts recording and the detection/QA loop.
    pub async fn start_recording(
        &self,
        project_id: String,
        session_id: String,
    ) -> Result<(
        broadcast::Receiver<UiChunk>,
        broadcast::Receiver<StableSegment>,
        broadcast::Receiver<String>, // Streaming LLM tokens/answers
    )> {
        let db = &self.state.db;

        // 1. Fetch settings for model paths
        let settings = settings::Entity::find_by_id("default")
            .one(db)
            .await
            .map_err(|e| anyhow!("Failed to fetch settings: {}", e))?
            .ok_or_else(|| anyhow!("Settings not found"))?;

        let moonshine_path = install::moonshine_path(&settings.transcript_model, None);
        let silero_path = install::default_silero_path(None);

        if !moonshine_path.exists() || !silero_path.exists() {
            return Err(anyhow::anyhow!(
                "Required STT/VAD models are currently missing. Please wait for the initial download to complete, or restart the app."
            ));
        }

        let moonshine_str = moonshine_path
            .to_str()
            .ok_or_else(|| anyhow!("Moonshine model path contains invalid UTF-8"))?;

        // 2. Initialize AudioRuntime
        let mut runtime_guard = self.state.audio_runtime.lock().await;
        if runtime_guard.is_none() {
            let runtime = AudioRuntime::new(AudioSource::Monitor, moonshine_str)
                .map_err(|e| anyhow!("Failed to create audio runtime: {}", e))?;
            *runtime_guard = Some(runtime);
        }
        let Some(runtime) = runtime_guard.as_mut() else {
            return Err(anyhow!("AudioRuntime failed to initialize"));
        };
        runtime
            .start()
            .map_err(|e| anyhow!("Failed to start hardware capture: {}", e))?;

        let (qa_tx, qa_rx) = broadcast::channel::<String>(100);

        // 3. Initialize Embedder lazily
        // Initialize the intent detector exactly once
        let _ = self
            .state
            .detector
            .get_or_try_init(|| async {
                IntentDetector::new().map_err(|e| anyhow!("Failed to create detector: {}", e))
            })
            .await?;

        // 4. Initialize LLM if None
        let mut llm_guard = self.state.llm.lock().await;
        if llm_guard.is_none() {
            let api_key = crate::backend::models::llm::read_api_key()
                .map_err(|e| anyhow!("Failed to read API Key from secure storage: {}", e))?;
            let model = LlmModel::new(settings.llm_model.clone(), api_key);
            *llm_guard = Some(model);
        }
        drop(llm_guard);

        let ui_rx = runtime.ui_tx.subscribe();
        let segment_rx_for_db = runtime.segment_tx.subscribe();
        let segment_rx_for_qa = runtime.segment_tx.subscribe();
        let mut session_stop_rx_db = runtime.session_stop_tx.subscribe();
        let mut session_stop_rx_sum = runtime.session_stop_tx.subscribe();
        let mut session_stop_rx_qa = runtime.session_stop_tx.subscribe();

        let (notify_tx, mut notify_rx) = tokio::sync::mpsc::channel::<()>(100);

        // 5. Spawn DB write loop for transcripts
        let db_clone = db.clone();
        let session_id_clone = session_id.clone();
        let project_id_clone_for_db = project_id.clone();
        let mut rx = segment_rx_for_db;
        let notify_tx_clone = notify_tx.clone();
        spawn(async move {
            loop {
                tokio::select! {
                    segment_res = rx.recv() => {
                        match segment_res {
                            Ok(segment) => {
                                let duration_ms = segment.end_time.saturating_sub(segment.start_time) as i64;
                                let chunk = transcript_chunks::ActiveModel {
                                    id: Set(uuid::Uuid::new_v4().to_string()),
                                    session_id: Set(session_id_clone.clone()),
                                    project_id: Set(project_id_clone_for_db.clone()),
                                    chunk_index: Set(segment.start_time as i64),
                                    start_ms: Set(segment.start_time as i64),
                                    end_ms: Set(segment.end_time as i64),
                                    duration_ms: Set(duration_ms),
                                    text: Set(segment.full_text),
                                    created_at: Set(Utc::now()),
                                };
                                if let Err(e) = chunk.insert(&db_clone).await {
                                    error!("Failed to save transcript chunk: {}", e);
                                } else if let Err(e) = notify_tx_clone.send(()).await {
                                    tracing::error!("Failed to notify transcript summarizer: {:?}", e);
                                }
                            }
                            Err(_) => break,
                        }
                    }
                    _ = session_stop_rx_db.recv() => {
                        tracing::info!("DB write loop received stop signal. Exiting.");
                        break;
                    }
                }
            }
        });

        // 5.5 Spawn Transcript Summarizer Worker
        let db_summarizer = db.clone();
        let session_id_summarizer = session_id.clone();
        let project_id_summarizer = project_id.clone();
        let state_summarizer = self.state.clone();
        let settings_clone = settings.clone();

        spawn(async move {
            let target_duration_ms = settings_clone.summary_minutes * 60 * 1000;
            if target_duration_ms <= 0 {
                return; // Summarization disabled
            }

            loop {
                // Wait for a new chunk or channel closed (session ended/paused)
                let flush = tokio::select! {
                    val = notify_rx.recv() => {
                        val.is_none()
                    }
                    _ = session_stop_rx_sum.recv() => {
                        tracing::info!("Summarizer loop received stop signal. Exiting.");
                        break;
                    }
                };

                loop {
                    // Fetch the max summary index
                    let last_summary = transcript_summaries::Entity::find()
                        .filter(
                            transcript_summaries::Column::SessionId
                                .eq(session_id_summarizer.clone()),
                        )
                        .order_by_desc(transcript_summaries::Column::ChunkEndIndex)
                        .one(&db_summarizer)
                        .await
                        .unwrap_or(None);

                    let start_idx = last_summary
                        .as_ref()
                        .map(|s| s.chunk_end_index)
                        .unwrap_or(-1);
                    let summary_idx = last_summary
                        .as_ref()
                        .map(|s| s.summary_index + 1)
                        .unwrap_or(0);

                    // Fetch unprocessed chunks
                    let chunks = transcript_chunks::Entity::find()
                        .filter(
                            transcript_chunks::Column::SessionId.eq(session_id_summarizer.clone()),
                        )
                        .filter(transcript_chunks::Column::ChunkIndex.gt(start_idx))
                        .order_by_asc(transcript_chunks::Column::ChunkIndex)
                        .all(&db_summarizer)
                        .await
                        .unwrap_or_default();

                    if chunks.is_empty() {
                        break;
                    }

                    let total_dur: i64 = chunks.iter().map(|c| c.duration_ms).sum();

                    if total_dur >= target_duration_ms || flush {
                        // Group chunk texts
                        let combined_text = chunks
                            .iter()
                            .map(|c| c.text.clone())
                            .collect::<Vec<_>>()
                            .join(" ");
                        let Some(first_chunk) = chunks.first() else {
                            break;
                        };
                        let Some(last_chunk) = chunks.last() else {
                            break;
                        };
                        let chunk_start_index = first_chunk.chunk_index;
                        let chunk_end_index = last_chunk.chunk_index;
                        let start_ms = first_chunk.start_ms;
                        let end_ms = last_chunk.end_ms;

                        // LLM summary
                        let prompt = format!(
                            "Please summarize the following transcription segment concisely:\n\n{}",
                            combined_text
                        );

                        let mut summary_text = String::new();
                        let llm_opt = {
                            let llm_guard = state_summarizer.llm.lock().await;
                            llm_guard.clone()
                        };

                        if let Some(llm) = llm_opt {
                            use futures::StreamExt;
                            // Bypass context injection logic
                            if let Ok(mut stream) = llm.ask_stream(&prompt, vec![], 0.0, 0.0).await
                            {
                                while let Some(res) = stream.next().await {
                                    if let Ok(text) = res {
                                        summary_text.push_str(&text);
                                    }
                                }
                            }
                        }

                        if summary_text.is_empty() {
                            summary_text = combined_text; // Fallback if LLM fails
                        }

                        // Embedding
                        let mut vector_u8 = Vec::new();
                        let embedder_opt = state_summarizer.embedder.clone();
                        let summary_clone = summary_text.clone();

                        let vecs_res = tokio::task::spawn_blocking(move || {
                            if let Some(embedder) = embedder_opt.get()
                                && let Ok(mut vecs) = embedder.embed(vec![summary_clone])
                            {
                                return vecs.pop();
                            }
                            None
                        })
                        .await
                        .unwrap_or(None);

                        if let Some(v) = vecs_res {
                            vector_u8 = v.into_iter().flat_map(|f| f.to_le_bytes()).collect();
                        }

                        let new_summary = transcript_summaries::ActiveModel {
                            id: Set(uuid::Uuid::new_v4().to_string()),
                            project_id: Set(project_id_summarizer.clone()),
                            session_id: Set(session_id_summarizer.clone()),
                            summary_index: Set(summary_idx),
                            chunk_start_index: Set(chunk_start_index),
                            chunk_end_index: Set(chunk_end_index),
                            start_ms: Set(start_ms),
                            end_ms: Set(end_ms),
                            duration_ms: Set(total_dur),
                            summary: Set(summary_text),
                            vector: Set(vector_u8),
                            created_at: Set(Utc::now()),
                        };

                        if let Err(e) = new_summary.insert(&db_summarizer).await {
                            error!("Failed to save transcript summary: {}", e);
                            break; // Stop trying to avoid infinite loop
                        }
                    } else {
                        break;
                    }
                }

                if flush {
                    break;
                }
            }
        });

        // 6. Spawn Detection & QA loop
        let state_clone = self.state.clone();
        let session_id_clone2 = session_id.clone();
        let project_id_clone = project_id.clone();
        let mut qa_rx_loop = segment_rx_for_qa;
        let qa_tx_clone = qa_tx.clone();
        spawn(async move {
            let mut pending_segment: Option<StableSegment> = None;
            loop {
                tokio::select! {
                    segment_res = qa_rx_loop.recv() => {
                        let segment = match segment_res {
                            Ok(s) => s,
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                                tracing::warn!("QA loop lagged: skipped {} segments", skipped);
                                continue;
                            }
                            Err(_) => break,
                        };

                        let (text, is_paired) = if let Some(prev) = pending_segment.take() {
                            let combined = format!("{} {}", prev.full_text, segment.full_text);
                            (combined, true)
                        } else {
                            (segment.full_text.clone(), false)
                        };

                        // If this segment is not the end of speech, buffer it for the next pair
                        if !segment.is_end_of_speech {
                            pending_segment = Some(segment.clone());
                            if !is_paired {
                                continue;
                            }
                        }

                        // Execute CPU-bound detection and embedding in spawn_blocking
                        // to avoid blocking the Tokio async executor thread.
                        let embedder_cell = state_clone.embedder.clone();
                        let detector_cell = state_clone.detector.clone();
                        let text_clone = text.clone();

                        let blocking_result =
                            tokio::task::spawn_blocking(move || -> Result<Option<Vec<f32>>> {
                                let detector = detector_cell
                                    .get()
                                    .ok_or_else(|| anyhow!("Detector cell empty"))?;

                                let res = detector.detect(&text_clone)?;
                                tracing::debug!(
                                    "QA Stage 1: segment='{}' actionable={} similarity={:?}",
                                    text_clone,
                                    res.is_actionable,
                                    res.similarity
                                );

                                if !res.is_actionable {
                                    return Ok(None);
                                }

                                tracing::info!("QA Stage 2 passed! Actionable query detected: '{}'", text_clone);

                                // Only need the embedder once we know the query is actionable
                                let embedder = embedder_cell
                                    .get()
                                    .ok_or_else(|| anyhow!("Embedder not yet initialized"))?;
                                let mut vecs = embedder.embed(vec![text_clone])?;
                                Ok(vecs.pop())
                            })
                            .await;

                        let vector = match blocking_result {
                            Ok(Ok(Some(v))) => v,
                            Ok(Ok(None)) => continue, // Not actionable
                            Ok(Err(e)) => {
                                error!("QA pipeline error: {}", e);
                                continue;
                            }
                            Err(e) => {
                                error!("Spawn blocking error: {}", e);
                                continue;
                            }
                        };

                        // Step C: Search + LLM — no locks held across await
                        let db = &state_clone.db;

                        let send_qa = |msg: String| {
                            if let Err(e) = qa_tx_clone.send(msg) {
                                tracing::error!("Failed to broadcast QA token/status: {:?}", e);
                            }
                        };

                        send_qa(format!("[START_QA]:{}", text));

                        let (max_cosine, max_bm25, candidates) =
                            match smart_hybrid_search(db, &text, vector, Some(5)).await {
                                Ok(res) => res,
                                Err(e) => {
                                    error!("Search error: {}", e);
                                    send_qa(format!("Error: Context search failed: {}", e));
                                    continue;
                                }
                            };

                        // Clone LLM out of the lock so we don't hold it across .ask().await
                        let llm_opt = {
                            let llm_guard = state_clone.llm.lock().await;
                            llm_guard.clone()
                        };

                        let Some(llm) = llm_opt else {
                            error!("LLM not initialized");
                            send_qa(
                                "Error: LLM not initialized. Please configure your API key.".to_string(),
                            );
                            continue;
                        };

                        let question = format!(
                            "Based on this snippet, what should be done? ({})",
                            text
                        );
                        match llm
                            .ask_stream(&question, candidates, max_cosine, max_bm25)
                            .await
                        {
                            Ok(mut stream) => {
                                use futures::StreamExt;
                                let mut full_answer = String::new();
                                let mut last_send = tokio::time::Instant::now();
                                while let Some(chunk) = stream.next().await {
                                    if let Ok(text) = chunk {
                                        full_answer.push_str(&text);
                                        if last_send.elapsed().as_millis() > 500 {
                                            send_qa(full_answer.clone());
                                            last_send = tokio::time::Instant::now();
                                        }
                                    }
                                }

                                if full_answer.trim().is_empty() {
                                    send_qa("(LLM returned an empty response)".to_string());
                                } else {
                                    // Send the final result
                                    send_qa(full_answer.clone());

                                    let answer_model = session_answers::ActiveModel {
                                        id: Set(uuid::Uuid::new_v4().to_string()),
                                        session_id: Set(session_id_clone2.clone()),
                                        project_id: Set(project_id_clone.clone()),
                                        context: Set(None),
                                        query: Set(text.clone()),
                                        answer: Set(full_answer),
                                        created_at: Set(Utc::now()),
                                    };
                                    if let Err(e) = answer_model.insert(db).await {
                                        error!("Failed to save QA session: {}", e);
                                    }
                                }
                            }
                            Err(e) => {
                                error!("LLM QA error: {}", e);
                                send_qa(format!("LLM Error: {}", e));
                            }
                        }
                    }
                    _ = session_stop_rx_qa.recv() => {
                        tracing::info!("QA loop received stop signal. Exiting.");
                        break;
                    }
                }
            }
        });

        Ok((ui_rx, runtime.segment_tx.subscribe(), qa_rx))
    }

    /// Stops recording without dropping the AudioRuntime.
    pub async fn stop_recording(&self) -> Result<()> {
        let mut runtime_guard = self.state.audio_runtime.lock().await;
        if let Some(rt) = runtime_guard.as_mut() {
            rt.stop().context("Failed to stop audio runtime workers")?;
        }
        Ok(())
    }

    /// Ends the session, stops recording, and updates the DB timestamp.
    pub async fn end_session(&self, session_id: String) -> Result<()> {
        self.stop_recording().await?;

        let db = &self.state.db;

        // Fetch the session first to update it
        if let Some(session) = sessions::Entity::find_by_id(session_id.clone())
            .one(db)
            .await
            .map_err(|e| anyhow!("Failed to fetch session: {}", e))?
        {
            let mut active_session: sessions::ActiveModel = session.into();
            active_session.ended_at = Set(Some(Utc::now()));
            active_session.status = Set("completed".to_string());
            active_session.updated_at = Set(Utc::now());

            active_session
                .update(db)
                .await
                .map_err(|e| anyhow!("Failed to update session: {}", e))?;
        }

        // Explicitly drop/destroy the audio runtime on end of session
        {
            let mut runtime_guard = self.state.audio_runtime.lock().await;
            *runtime_guard = None;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use migration::MigratorTrait;

    #[tokio::test]
    async fn test_load_live_page() {
        let db = sea_orm::Database::connect("sqlite::memory:").await.unwrap();
        migration::Migrator::up(&db, None).await.unwrap();

        let state = AppState::new(db);
        let model = LivePageModel::new(state);
        let result = model.load("dummy").await;
        assert!(result.is_ok());
    }
}
