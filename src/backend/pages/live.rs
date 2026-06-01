use crate::backend::audio::runtime::AudioRuntime;
use crate::backend::audio::source::AudioSource;
use crate::backend::audio::transcript::{StableSegment, UiChunk};
use crate::backend::detection::IntentDetector;
use crate::backend::entities::{session_answers, sessions, settings, transcript_chunks};
use crate::backend::models::{install, llm::LlmModel};
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

        let is_ended = session.map(|s| s.status == "completed" || s.status == "cancelled").unwrap_or(false);

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

        // 3. Initialize intent detector exactly once
        self.state
            .detector
            .get_or_try_init(|| async {
                IntentDetector::new().map_err(|e| anyhow!("Failed to create detector: {}", e))
            })
            .await?;

        // 4. Initialize LLM if None
        self.state
            .llm
            .get_or_try_init(|| async {
                let api_key = crate::backend::models::llm::read_api_key()
                    .map_err(|e| anyhow!("Failed to read API Key from secure storage: {}", e))?;
                Ok::<LlmModel, anyhow::Error>(LlmModel::new(settings.llm_model.clone(), api_key))
            })
            .await?;

        let session_graph = std::sync::Arc::new(std::sync::RwLock::new(
            crate::backend::graph::SessionGraph::new(),
        ));

        let ui_rx = runtime.ui_tx.subscribe();
        let segment_rx_for_db = runtime.segment_tx.subscribe();
        let segment_rx_for_qa = runtime.segment_tx.subscribe();
        let segment_rx_for_graph = runtime.segment_tx.subscribe();
        let mut session_stop_rx_db = runtime.session_stop_tx.subscribe();
        let mut session_stop_rx_qa = runtime.session_stop_tx.subscribe();
        let mut session_stop_rx_graph = runtime.session_stop_tx.subscribe();

        // 5. Spawn DB write loop for transcripts & GraphRAG extraction
        let db_clone = db.clone();
        let session_id_clone = session_id.clone();
        let project_id_clone_for_db = project_id.clone();
        let mut rx = segment_rx_for_db;

        spawn(async move {
            loop {
                tokio::select! {
                    segment_res = rx.recv() => {
                        match segment_res {
                            Ok(segment) => {
                                let duration_ms = segment.end_time.saturating_sub(segment.start_time) as i64;
                                let chunk = transcript_chunks::ActiveModel {
                                    id: Set(segment.id.to_string()),
                                    session_id: Set(session_id_clone.clone()),
                                    project_id: Set(project_id_clone_for_db.clone()),
                                    chunk_index: Set(segment.start_time as i64),
                                    start_ms: Set(segment.start_time as i64),
                                    end_ms: Set(segment.end_time as i64),
                                    duration_ms: Set(duration_ms),
                                    text: Set(segment.full_text.clone()),
                                    created_at: Set(Utc::now()),
                                };
                                if let Err(e) = chunk.insert(&db_clone).await {
                                    error!("Failed to save transcript chunk: {}", e);
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

        // 5.5 Spawn GraphRAG processing loop
        let state_clone_for_graph = self.state.clone();
        let session_graph_for_graph = session_graph.clone();
        let mut rx_graph = segment_rx_for_graph;
        spawn(async move {
            loop {
                tokio::select! {
                    segment_res = rx_graph.recv() => {
                        match segment_res {
                            Ok(segment) => {
                                let state_for_block = state_clone_for_graph.clone();
                                let text_to_process = segment.full_text.clone();
                                let chunk_id = segment.id;
                                let session_graph_clone = session_graph_for_graph.clone();
                                tokio::task::spawn_blocking(move || {
                                    if let Some(pos_model) = state_for_block.pos_model.get() {
                                        match pos_model.extract_entities(&text_to_process) {
                                            Ok(entities) => {
                                                if entities.is_empty() { return; }

                                                tracing::info!("Extracted {} semantic entities from chunk", entities.len());

                                                let mut graph = match session_graph_clone.write() {
                                                    Ok(g) => g,
                                                    Err(e) => {
                                                        tracing::error!("Graph RwLock poisoned: {}", e);
                                                        return;
                                                    }
                                                };

                                                if entities.len() == 1 {
                                                    let entity = &entities[0];
                                                    graph.add_node(&entity.text, &entity.category, chunk_id);
                                                    tracing::debug!("Added 1 isolated node to Session Graph: {}", entity.text);
                                                    return;
                                                }

                                                let mut edge_count = 0;
                                                for i in 0..entities.len() - 1 {
                                                    let source = &entities[i];
                                                    let target = &entities[i + 1];
                                                    graph.add_edge(
                                                        &source.text,
                                                        &source.category,
                                                        &target.text,
                                                        &target.category,
                                                        chunk_id,
                                                    );
                                                    edge_count += 1;
                                                }
                                                tracing::debug!("Added {} new semantic edges to Session Graph", edge_count);
                                            }
                                            Err(e) => tracing::error!("Failed to extract entities: {:?}", e),
                                        }
                                    }
                                });
                            }
                            Err(_) => break,
                        }
                    }
                    _ = session_stop_rx_graph.recv() => {
                        tracing::info!("Graph loop received stop signal. Exiting.");
                        break;
                    }
                }
            }
        });

        // 6. Spawn Detection & QA loop using GraphRAG context
        let state_clone = self.state.clone();
        let session_id_clone2 = session_id.clone();
        let project_id_clone = project_id.clone();
        let session_graph_for_qa = session_graph.clone();
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

                        // Execute CPU-bound detection in spawn_blocking
                        let detector_cell = state_clone.detector.clone();
                        let text_clone = text.clone();

                        let blocking_result =
                            tokio::task::spawn_blocking(move || -> Result<Option<String>> {
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

                                Ok(res.extracted_query)
                            })
                            .await;

                        let extracted_query = match blocking_result {
                            Ok(Ok(Some(query))) => query,
                            Ok(Ok(None)) => continue,
                            Ok(Err(e)) => {
                                error!("QA pipeline error: {}", e);
                                continue;
                            }
                            Err(e) => {
                                error!("Spawn blocking error: {}", e);
                                continue;
                            }
                        };

                        tracing::info!("QA Stage 2 passed! Actionable query detected: '{}'", extracted_query);

                        let db = &state_clone.db;

                        let send_qa = |msg: String| {
                            if let Err(e) = qa_tx_clone.send(msg) {
                                tracing::error!("Failed to broadcast QA token/status: {:?}", e);
                            }
                        };
                        
                        // Immediately show Thinking state on the UI
                        send_qa(format!("[START_QA]:{}", text));

                        // 1. Get graph facts
                        let facts = match session_graph_for_qa.read() {
                            Ok(graph) => graph.find_matches(&extracted_query),
                            Err(e) => {
                                tracing::error!("Session graph poisoned: {}", e);
                                vec![]
                            }
                        };

                        let mut all_chunk_ids = std::collections::HashSet::new();
                        for fact in &facts {
                            for id in &fact.chunk_ids {
                                all_chunk_ids.insert(id.to_string());
                            }
                        }

                        let mut chunk_map = std::collections::HashMap::new();
                        if !all_chunk_ids.is_empty() {
                            match transcript_chunks::Entity::find()
                                .filter(transcript_chunks::Column::Id.is_in(all_chunk_ids))
                                .all(db)
                                .await
                            {
                                Ok(chunks) => {
                                    for c in chunks {
                                        chunk_map.insert(c.id, c.text);
                                    }
                                }
                                Err(e) => tracing::error!("Failed to fetch graph chunks: {}", e),
                            }
                        }

                        let mut has_graph_facts = false;
                        let mut candidates: Vec<crate::backend::search::SearchCandidate> = facts
                            .into_iter()
                            .enumerate()
                            .map(|(i, fact)| {
                                has_graph_facts = true;
                                let mut context_texts = Vec::new();
                                for id in &fact.chunk_ids {
                                    if let Some(text) = chunk_map.get(&id.to_string()) {
                                        context_texts.push(text.clone());
                                    }
                                }

                                let content = if context_texts.is_empty() {
                                    fact.fact
                                } else {
                                    format!("{} | Context: {}", fact.fact, context_texts.join(" ... "))
                                };

                                crate::backend::search::SearchCandidate {
                                    id: format!("graph_fact_{}", i),
                                    source_type: "graph_relation".to_string(),
                                    content,
                                    raw_score: 1.0, // High raw score since it's an exact graph match
                                }
                            })
                            .collect();

                        let mut max_cosine = 0.0;
                        let mut max_bm25 = 0.0;

                        // 2. Fetch document chunk vectors using Hybrid Search
                        let embedder_arc = state_clone.embedder.clone();
                        let text_to_embed = extracted_query.clone();
                        let embed_result = tokio::task::spawn_blocking(move || {
                            if let Some(embedder) = embedder_arc.get() {
                                embedder.embed(vec![text_to_embed])
                            } else {
                                Err(anyhow::anyhow!("Embedder not initialized"))
                            }
                        })
                        .await;

                        if let Ok(Ok(vectors)) = embed_result {
                            if let Some(query_vector) = vectors.into_iter().next() {
                                match crate::backend::search::smart_hybrid_search(
                                    db,
                                    &extracted_query,
                                    query_vector,
                                    Some(5), // limit
                                )
                                .await
                                {
                                    Ok((c_max, b_max, doc_candidates)) => {
                                        max_cosine = c_max;
                                        max_bm25 = b_max;
                                        candidates.extend(doc_candidates);
                                    }
                                    Err(e) => {
                                        tracing::error!("Hybrid document search failed: {}", e);
                                    }
                                }
                            }
                        } else if let Err(e) = embed_result {
                            tracing::error!("Embedder task failed: {}", e);
                        }

                        // If we have highly-accurate graph facts, boost the scores so it triggers
                        // the strict, anti-hallucination prompt block.
                        if has_graph_facts {
                            if max_cosine < 0.8 {
                                max_cosine = 0.8;
                            }
                            if max_bm25 < 6.0 {
                                max_bm25 = 6.0;
                            }
                        } else if max_cosine == 0.0 && max_bm25 == 0.0 && !candidates.is_empty() {
                            max_cosine = 1.0;
                            max_bm25 = 10.0;
                        }

                        let llm_opt = state_clone.llm.get().cloned();

                        let Some(llm) = llm_opt else {
                            error!("LLM not initialized");
                            send_qa(format!("[START_QA]:{}", extracted_query));
                            send_qa(
                                "Error: LLM not initialized. Please configure your API key.".to_string(),
                            );
                            continue;
                        };

                        let question = format!(
                            "Based on this snippet, what should be done? ({})",
                            extracted_query
                        );
                        match llm
                            .ask_stream(&question, candidates, max_cosine, max_bm25)
                            .await
                        {
                            Ok(mut stream) => {
                                use futures::StreamExt;
                                let mut full_response = String::new();
                                let mut ignored = false;

                                while let Some(chunk) = stream.next().await {
                                    if let Ok(chunk_text) = chunk {
                                        full_response.push_str(&chunk_text);

                                        // Buffer if it looks like it might be writing [IGNORE]
                                        if full_response.len() < 8 && "[IGNORE]".starts_with(full_response.trim()) {
                                            continue;
                                        }

                                        if full_response.contains("[IGNORE]") {
                                            send_qa("[IGNORE]".to_string());
                                            ignored = true;
                                            break;
                                        }

                                        send_qa(full_response.clone());
                                    }
                                }

                                if ignored || full_response.trim() == "[IGNORE]" {
                                    tracing::info!("QA Loop: Ignored speech segment as noise/non-actionable: '{}'", text);
                                } else if !full_response.is_empty() {
                                    let answer_model = session_answers::ActiveModel {
                                        id: Set(uuid::Uuid::new_v4().to_string()),
                                        session_id: Set(session_id_clone2.clone()),
                                        project_id: Set(project_id_clone.clone()),
                                        context: Set(None),
                                        query: Set(text.clone()),
                                        answer: Set(full_response.trim().to_string()),
                                        created_at: Set(Utc::now()),
                                    };
                                    if let Err(e) = answer_model.insert(db).await {
                                        tracing::error!("Failed to save QA session: {}", e);
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::error!("LLM QA error: {}", e);
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
