use anyhow::Result;
use crossbeam_channel::{Receiver, Sender, unbounded};
use embedding::Embedder;
use futures_util::StreamExt;
use keyring::Entry;
#[cfg(not(test))]
use keyring::Error as KeyringError;
use llm::{ChatStream, LlmClient};
use migration::{Migrator, MigratorTrait};
use reqwest::{StatusCode, blocking::Client, header::RANGE};
use sea_orm::{ActiveModelTrait, ColumnTrait, Database, EntityTrait, QueryFilter, QueryOrder, Set};
use sha1::{Digest, Sha1};
use std::path::PathBuf;
use std::sync::Arc;
use storage::{
    entities::{documents, projects, sessions, settings, transcript_chunks, transcript_summaries},
    lance_summary::LanceTranscriptSummary,
    path::{StorageKind, default_storage_path, ensure_path_exists},
    storage::Storage,
};
use stt::chunk::TranscriptionChunk;

pub use embedding::{DENSE_MODEL_OPTIONS, RERANKER_MODEL_OPTIONS, SPARSE_MODEL_OPTIONS};

use crate::{
    AudioRuntime, project,
    retrieval::{self, RetrievalHit},
    session, settings as settings_service, transcript,
    upload::{self, UploadDocumentOptions, UploadedDocument},
};

#[derive(Clone, Debug, PartialEq, Eq)]
/// Project data shaped for the UI.
///
/// This intentionally hides SeaORM timestamps and nullable database fields. The
/// dashboard only needs a stable id, display name, and display description; the
/// facade normalizes `NULL` descriptions to an empty string so components do not
/// need database-specific branching.
pub struct ProjectView {
    pub id: String,
    pub name: String,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Document row shown in the project detail document list.
///
/// SQLite remains the source of truth for uploaded document metadata. This view
/// presents the original filename, a user-readable path/status, and a simple
/// kind string derived from the extension. LanceDB chunks are intentionally not
/// exposed here because they are an internal retrieval projection.
pub struct DocumentView {
    pub id: String,
    pub name: String,
    pub meta: String,
    pub kind: String,
    pub status: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Session row shown in the project detail session rail.
///
/// The UI only needs an id, a title, and a compact metadata line. Start/end
/// timestamps and status remain in storage entities so session lifecycle logic
/// stays centralized in `session.rs` and the facade methods below.
pub struct SessionView {
    pub id: String,
    pub title: String,
    pub status: String,
    pub meta: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Transcript chunk data consumed by the live session screen.
///
/// The `has_question` and `confidence` fields are derived by the detector when
/// a chunk is persisted, and recomputed when historical chunks are loaded. This
/// makes the live page deterministic even if it is reopened after app restart.
pub struct TranscriptChunkView {
    pub id: String,
    pub text: String,
    pub time: String,
    pub chunk_index: i64,
    pub start_ms: i64,
    pub end_ms: i64,
    pub duration_ms: i64,
    pub has_question: bool,
    pub confidence: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Settings data safe for rendering in the UI.
///
/// The API key is never returned. `llm_api_key_preview` is a masked string built
/// from the OS keyring value and `has_llm_api_key` is a boolean convenience for
/// flows that must block recording until answer generation is configured.
pub struct SettingsView {
    pub llm_provider: String,
    pub llm_model: String,
    pub llm_api_key_preview: String,
    pub has_llm_api_key: bool,
    pub embedding_provider: String,
    pub embedding_model: String,
    pub sparse_model: String,
    pub reranker_model: String,
    pub whisper_model: String,
    pub summary_minutes: i64,
    pub ui_theme: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Settings update command from the UI.
///
/// `new_llm_api_key` and `remove_llm_api_key` model replace/delete semantics for
/// a single OS-keyring credential. Passing both remove=true and a non-empty new
/// key first deletes the old credential and then stores the new key, which keeps
/// replacement deterministic and avoids stale secrets.
pub struct SettingsUpdate {
    pub llm_model: String,
    pub new_llm_api_key: Option<String>,
    pub remove_llm_api_key: bool,
    pub embedding_model: String,
    pub sparse_model: String,
    pub reranker_model: String,
    pub whisper_model: String,
    pub summary_minutes: i64,
    pub ui_theme: String,
}

/// UI form model for settings screens.
///
/// This is still a data type, not Dioxus state. The UI can hold it in signals,
/// but defaults, mapping from saved settings, and conversion to update commands
/// live in core so form behavior stays consistent across pages/tests.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettingsFormView {
    pub llm_provider: String,
    pub llm_model: String,
    pub llm_api_key_preview: String,
    pub new_llm_api_key: String,
    pub remove_llm_api_key: bool,
    pub embedding_provider: String,
    pub embedding_model: String,
    pub sparse_model: String,
    pub reranker_model: String,
    pub whisper_model: String,
    pub summary_minutes: String,
    pub ui_theme: String,
}

/// Local Whisper model state for the Settings manage dialog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WhisperModelView {
    pub model_name: String,
    pub path: String,
    pub installed: bool,
}

/// OpenAI-compatible stream plus the retrieval context that produced it.
///
/// The UI consumes `stream` incrementally so the first answer bytes appear as
/// soon as the model sends them. After the stream completes, the UI calls
/// `persist_answer` with the same `context` so stored answers are auditable.
pub struct AnswerStreamView {
    pub context: String,
    pub stream: ChatStream,
}

/// Events emitted by the core-owned live session pipeline.
///
/// Dioxus should treat this as render data only. Core owns audio capture,
/// transcript persistence, question detection, retrieval, LLM streaming, and
/// answer persistence; the UI only updates signals when events arrive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LiveSessionEvent {
    Transcript(TranscriptChunkView),
    QuestionDetected { question: String, confidence: u8 },
    AnswerStarted,
    AnswerDelta(String),
    AnswerFinished(String),
    Error(String),
}

/// Running live session pipeline returned to the UI.
///
/// The handle owns the audio runtime and an event receiver. UI code should store
/// this handle in state, poll `events`, and pass the handle back to core for
/// pause/end. It should not call `AudioRuntime` directly.
pub struct LiveSessionHandle {
    runtime: Option<AudioRuntime>,
    events: Receiver<LiveSessionEvent>,
}

impl LiveSessionHandle {
    /// Clone the event receiver so a UI task can poll core-produced events.
    pub fn events(&self) -> Receiver<LiveSessionEvent> {
        self.events.clone()
    }

    fn stop_detached(mut self) {
        if let Some(mut runtime) = self.runtime.take() {
            std::thread::spawn(move || runtime.stop());
        }
    }
}

/// Shared application service facade used by UI and workers.
///
/// `TayoriCore` owns one `Arc<Storage>` and exposes higher-level operations that
/// compose storage, embedding, detection, retrieval, and LLM crates. Keep this
/// type as the boundary between Dioxus and persistence so the app does not open
/// duplicate storage handles or depend on generated SeaORM entity shapes.
#[derive(Clone)]
pub struct TayoriCore {
    storage: Arc<Storage>,
}

impl TayoriCore {
    /// Run SQLite migrations, open storage, and ensure LanceDB tables/indexes.
    ///
    /// This is the normal desktop startup path. It first opens SQLite directly
    /// to run migrations, drops that temporary connection, then constructs
    /// `Storage::new()` so the long-lived storage handle owns both SQLite and
    /// LanceDB. That order avoids the UI creating separate storage instances.
    pub async fn bootstrap() -> Result<Self> {
        let sqlite_path = default_storage_path(StorageKind::Sqlite)?;
        ensure_path_exists(StorageKind::Sqlite, &sqlite_path)?;

        let sqlite_uri = format!("sqlite://{sqlite_path}?mode=rwc");
        let sqlite = Database::connect(sqlite_uri).await?;
        Migrator::up(&sqlite, None).await?;
        drop(sqlite);

        let storage = Storage::new().await?;
        Ok(Self {
            storage: Arc::new(storage),
        })
    }

    /// Load user-editable settings plus masked keyring state.
    ///
    /// The returned API key fields are safe for UI rendering. If the OS keyring
    /// cannot be read, the preview is empty and `has_llm_api_key` is false;
    /// callers should use that to show a setup prompt before recording.
    pub async fn settings(&self) -> Result<SettingsView> {
        settings_service::get_settings(&self.storage)
            .await
            .map(settings_view)
    }

    /// Load settings as an editable UI form model.
    pub async fn settings_form(&self) -> Result<SettingsFormView> {
        self.settings().await.map(settings_form_view)
    }

    /// Persist settings changes and apply single-key OS keyring semantics.
    ///
    /// Non-secret settings are written to SQLite. The LLM API key is handled via
    /// the OS keyring only: remove deletes the credential, and a non-empty new
    /// key replaces any previous credential. SQLite stores only a marker ref so
    /// future migrations can tell that the key belongs in keyring.
    pub async fn update_settings(&self, update: SettingsUpdate) -> Result<SettingsView> {
        let mut model = settings_service::get_settings(&self.storage).await?;
        let search_models_changed = model.embedding_model != update.embedding_model
            || model.sparse_model != update.sparse_model;
        let reranker_model_changed = model.reranker_model != update.reranker_model;
        let documents_to_rebuild = if search_models_changed {
            self.documents_to_rebuild().await?
        } else {
            Vec::new()
        };
        Embedder::validate_models(
            &update.embedding_model,
            &update.sparse_model,
            &update.reranker_model,
        )?;
        let embedding_dimension = Embedder::dense_model_dimension(&update.embedding_model)?;
        let previous_whisper_model = model
            .whisper_model
            .clone()
            .unwrap_or_else(|| stt::path::DEFAULT_WHISPER_MODEL_NAME.to_string());
        let next_whisper_model = optional_string(update.whisper_model.clone())
            .unwrap_or_else(|| stt::path::DEFAULT_WHISPER_MODEL_NAME.to_string());
        validate_whisper_model_name(&next_whisper_model)?;

        model.llm_model = update.llm_model;
        model.llm_store_responses = 1;
        model.embedding_model = update.embedding_model;
        model.sparse_model = update.sparse_model;
        model.embedding_dimension = embedding_dimension as i64;
        model.reranker_model = update.reranker_model;
        model.whisper_model = Some(next_whisper_model.clone());
        model.summary_minutes = update.summary_minutes.clamp(1, 10);
        model.ui_theme = update.ui_theme;

        if previous_whisper_model != next_whisper_model {
            install_whisper_model(&next_whisper_model)?;
        }

        if update.remove_llm_api_key {
            delete_llm_api_key()?;
            model.llm_api_key_ref = None;
        }

        if let Some(api_key) = update.new_llm_api_key.and_then(optional_string) {
            replace_llm_api_key(&api_key)?;
            model.llm_api_key_ref = Some(LLM_API_KEY_REF.to_string());
        }

        let mut prepared_embedder = if search_models_changed || reranker_model_changed {
            Some(embedder_from_settings(&model)?)
        } else {
            None
        };

        let model = settings_service::update_settings(&self.storage, model).await?;
        if search_models_changed {
            let embedder = prepared_embedder
                .as_mut()
                .ok_or_else(|| anyhow::anyhow!("embedding models were not prepared"))?;
            self.rebuild_search_data(embedder, documents_to_rebuild)
                .await?;
        }
        Ok(settings_view(model))
    }

    /// Save settings from an editable form model and return the refreshed form.
    pub async fn update_settings_form(&self, form: SettingsFormView) -> Result<SettingsFormView> {
        self.update_settings(settings_update_from_form_view(form))
            .await
            .map(settings_form_view)
    }

    /// Return whether a named Whisper model exists in Tayori's model directory.
    pub fn whisper_model_status(&self, model_name: &str) -> Result<WhisperModelView> {
        validate_whisper_model_name_or_blank(model_name)?;
        whisper_model_view(model_name)
    }

    /// List all supported Whisper models with local install state.
    pub fn whisper_models(&self) -> Result<Vec<WhisperModelView>> {
        WHISPER_MODELS
            .iter()
            .map(|model| whisper_model_view(model.name))
            .collect()
    }

    /// Install a named Whisper model through the Rust-owned script wrapper.
    pub fn install_whisper_model_by_name(&self, model_name: &str) -> Result<WhisperModelView> {
        validate_whisper_model_name(model_name)?;
        install_whisper_model(model_name)?;
        whisper_model_view(model_name)
    }

    /// Remove a named local Whisper model file if it exists.
    ///
    /// This only deletes the selected model file under Tayori's model directory;
    /// it does not alter saved settings. The UI can remove then save a different
    /// model name if the user wants to switch defaults.
    pub fn remove_whisper_model_by_name(&self, model_name: &str) -> Result<WhisperModelView> {
        validate_whisper_model_name(model_name)?;
        let path = stt::path::model_path(model_name);
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        whisper_model_view(model_name)
    }

    /// Create a project and return the UI-facing projection.
    pub async fn create_project(
        &self,
        name: impl Into<String>,
        description: Option<String>,
    ) -> Result<ProjectView> {
        project::create_project(&self.storage, name, description)
            .await
            .map(project_view)
    }

    /// List projects newest-first for the dashboard.
    pub async fn list_projects(&self) -> Result<Vec<ProjectView>> {
        Ok(project::list_projects(&self.storage)
            .await?
            .into_iter()
            .map(project_view)
            .collect())
    }

    /// Delete a project and its rebuildable search projection.
    pub async fn delete_project(&self, project_id: &str) -> Result<()> {
        self.storage
            .delete_lance_document_chunks_for_project(project_id)
            .await?;
        self.storage
            .delete_lance_transcript_summaries_for_project(project_id)
            .await?;
        project::delete_project(&self.storage, project_id).await
    }

    /// Create a running session for a project.
    ///
    /// A session stays `running` until `end_session` is called. UI Start/Pause
    /// controls only the audio listener; they must not change this DB status.
    pub async fn create_session(
        &self,
        project_id: impl Into<String>,
        title: Option<String>,
    ) -> Result<SessionView> {
        session::create_session(&self.storage, project_id, title)
            .await
            .map(session_view)
    }

    /// List sessions for one project newest-first.
    pub async fn list_sessions(&self, project_id: &str) -> Result<Vec<SessionView>> {
        Ok(session::list_sessions(&self.storage, project_id)
            .await?
            .into_iter()
            .map(session_view)
            .collect())
    }

    /// Mark a session completed.
    ///
    /// Missing session ids are treated as no-ops by `session::end_session`, which
    /// keeps End idempotent when UI events race with navigation or reloads.
    pub async fn end_session(&self, session_id: &str) -> Result<()> {
        session::end_session(&self.storage, session_id).await
    }

    /// Delete one session and its rebuildable transcript summary projection.
    pub async fn delete_session(&self, project_id: &str, session_id: &str) -> Result<()> {
        self.storage
            .delete_lance_transcript_summaries_for_session(project_id, session_id)
            .await?;
        session::delete_session(&self.storage, session_id).await
    }

    /// List SQLite document metadata for a project.
    pub async fn list_documents(&self, project_id: &str) -> Result<Vec<DocumentView>> {
        Ok(upload::list_documents(&self.storage, project_id)
            .await?
            .into_iter()
            .map(document_view)
            .collect())
    }

    /// Delete one uploaded document and its rebuildable search chunks.
    pub async fn delete_document(&self, project_id: &str, document_id: &str) -> Result<()> {
        self.storage
            .delete_lance_document_chunks_for_document(project_id, document_id)
            .await?;
        upload::delete_document(&self.storage, document_id).await
    }

    /// Upload using an already constructed embedder.
    ///
    /// Workers/tests can use this when they need explicit embedder lifetime
    /// control. UI code generally uses `upload_document_from_settings` instead.
    pub async fn upload_document(
        &self,
        embedder: &mut Embedder,
        project_id: String,
        path: impl AsRef<std::path::Path>,
    ) -> Result<UploadedDocument> {
        upload::upload_document(&self.storage, embedder, project_id, path).await
    }

    /// Upload a local document using embedding settings from SQLite.
    ///
    /// The source file is not copied. Metadata goes to SQLite and chunk vectors
    /// go to LanceDB. This method returns the freshly uploaded document view so
    /// the UI can update the list without importing storage entities.
    pub async fn upload_document_from_settings(
        &self,
        project_id: String,
        path: impl AsRef<std::path::Path>,
    ) -> Result<DocumentView> {
        let settings = settings_service::get_settings(&self.storage).await?;
        let mut embedder = embedder_from_settings(&settings)?;
        let uploaded =
            upload::upload_document(&self.storage, &mut embedder, project_id, path).await?;
        let document = documents::Entity::find_by_id(uploaded.document_id)
            .one(&self.storage.sqlite)
            .await?
            .ok_or_else(|| anyhow::anyhow!("uploaded document row not found"))?;
        Ok(document_view(document))
    }

    /// Upload with explicit chunking options and an existing embedder.
    pub async fn upload_document_with_options(
        &self,
        embedder: &mut Embedder,
        project_id: String,
        path: impl AsRef<std::path::Path>,
        options: UploadDocumentOptions,
    ) -> Result<UploadedDocument> {
        upload::upload_document_with_options(&self.storage, embedder, project_id, path, options)
            .await
    }

    /// Persist a Whisper transcript chunk and return detector metadata.
    ///
    /// This is called by the live audio loop. It stores raw transcript text in
    /// SQLite, runs question/command detection, and returns a view that the UI
    /// can append to the transcript rail immediately.
    pub async fn persist_transcription_chunk(
        &self,
        project_id: &str,
        session_id: &str,
        chunk: TranscriptionChunk,
    ) -> Result<TranscriptChunkView> {
        transcript::persist_transcription_chunk(&self.storage, project_id, session_id, chunk)
            .await
            .map(|stored| transcript_chunk_view(stored.chunk, stored.detection))
    }

    /// Load transcript chunks for an existing session.
    ///
    /// Detection metadata is recomputed on read so reopening a live session has
    /// the same question highlighting behavior as live ingestion.
    pub async fn list_transcript_chunks(
        &self,
        session_id: &str,
    ) -> Result<Vec<TranscriptChunkView>> {
        Ok(
            transcript::list_transcript_chunks(&self.storage, session_id)
                .await?
                .into_iter()
                .map(|chunk| {
                    let detection = detection::detect_question_or_command(&chunk.text);
                    transcript_chunk_view(chunk, detection)
                })
                .collect(),
        )
    }

    /// Run retrieval and open a streaming answer request using saved settings.
    ///
    /// Flow: read settings -> read OS keyring API key -> construct embedder ->
    /// hybrid search + rerank -> format context -> call LLM streaming answer
    /// prompt. The returned stream is not persisted here because the UI needs to
    /// display deltas as they arrive; persist after the stream completes.
    pub async fn stream_answer_from_settings(
        &self,
        project_id: &str,
        question: &str,
    ) -> Result<AnswerStreamView> {
        let settings = settings_service::get_settings(&self.storage).await?;
        let api_key = llm_api_key()?;

        let mut embedder = embedder_from_settings(&settings)?;
        let hits = retrieval::retrieve_project_context(
            &self.storage,
            &mut embedder,
            project_id,
            question,
            8,
        )
        .await?;
        let context = retrieval::format_context(&hits);
        let llm = llm_from_settings(&settings, api_key);
        let stream = llm
            .stream_answer_from_context(&settings.llm_model, question, &context)
            .await?;

        Ok(AnswerStreamView { context, stream })
    }

    /// Start the full live meeting pipeline and return an event handle.
    ///
    /// This method is the high-level boundary for the Start button. It starts
    /// local audio/STT, persists transcript chunks, detects questions, performs
    /// hybrid retrieval/reranking, streams LLM answer deltas, and persists the
    /// completed answer. UI code should only render `LiveSessionEvent`s.
    pub fn start_live_session(
        &self,
        project_id: String,
        session_id: String,
    ) -> Result<LiveSessionHandle> {
        let mut runtime = AudioRuntime::start_default()?;
        let transcriptions = runtime.transcriptions();
        let (event_tx, event_rx) = unbounded();
        runtime.start()?;

        let core = self.clone();
        std::thread::spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(error) => {
                    let _ = event_tx.send(LiveSessionEvent::Error(error.to_string()));
                    return;
                }
            };

            let settings = match rt.block_on(settings_service::get_settings(&core.storage)) {
                Ok(settings) => settings,
                Err(error) => {
                    let _ = event_tx.send(LiveSessionEvent::Error(error.to_string()));
                    return;
                }
            };
            let mut summarizer = TranscriptSummaryAccumulator::new(settings.summary_minutes);
            let mut question_detector = RollingQuestionDetector::new();

            while let Ok(chunk) = transcriptions.recv() {
                let should_continue = rt.block_on(core.process_live_chunk(
                    &project_id,
                    &session_id,
                    chunk,
                    &event_tx,
                    &mut summarizer,
                    &mut question_detector,
                ));

                if !should_continue {
                    break;
                }
            }

            rt.block_on(core.flush_summary_window(
                &project_id,
                &session_id,
                &mut summarizer,
                &event_tx,
            ));
        });

        Ok(LiveSessionHandle {
            runtime: Some(runtime),
            events: event_rx,
        })
    }

    /// Stop audio capture without changing the database session status.
    ///
    /// This is the Pause button behavior. The session remains `running` so it
    /// can be resumed by starting a new live handle for the same session id.
    pub fn pause_live_session(&self, handle: LiveSessionHandle) {
        handle.stop_detached();
    }

    /// Stop audio capture and mark the session completed.
    ///
    /// Stopping audio is detached so End does not block the UI while Whisper is
    /// finishing a decode. Database completion is still awaited and reported to
    /// the caller.
    pub async fn end_live_session(
        &self,
        handle: Option<LiveSessionHandle>,
        session_id: &str,
    ) -> Result<()> {
        if let Some(handle) = handle {
            handle.stop_detached();
        }
        self.end_session(session_id).await
    }

    async fn process_live_chunk(
        &self,
        project_id: &str,
        session_id: &str,
        chunk: TranscriptionChunk,
        event_tx: &Sender<LiveSessionEvent>,
        summarizer: &mut TranscriptSummaryAccumulator,
        question_detector: &mut RollingQuestionDetector,
    ) -> bool {
        let chunk = match self
            .persist_transcription_chunk(project_id, session_id, chunk)
            .await
        {
            Ok(chunk) => chunk,
            Err(error) => {
                return event_tx
                    .send(LiveSessionEvent::Error(error.to_string()))
                    .is_ok();
            }
        };

        if event_tx
            .send(LiveSessionEvent::Transcript(chunk.clone()))
            .is_err()
        {
            return false;
        }

        let should_summarize = summarizer.push(chunk.clone());
        let detected_question = question_detector.push(chunk.clone());

        if let Some(question) = detected_question
            && !self
                .answer_detected_question(project_id, session_id, question, event_tx)
                .await
        {
            return false;
        }

        if should_summarize {
            self.flush_summary_window(project_id, session_id, summarizer, event_tx)
                .await;
        }

        true
    }

    async fn answer_detected_question(
        &self,
        project_id: &str,
        session_id: &str,
        question: DetectedQuestionWindow,
        event_tx: &Sender<LiveSessionEvent>,
    ) -> bool {
        if event_tx
            .send(LiveSessionEvent::QuestionDetected {
                question: question.text.clone(),
                confidence: question.confidence,
            })
            .is_err()
        {
            return false;
        }

        if event_tx.send(LiveSessionEvent::AnswerStarted).is_err() {
            return false;
        }

        let answer_stream = match self
            .stream_answer_from_settings(project_id, &question.text)
            .await
        {
            Ok(answer_stream) => answer_stream,
            Err(error) => {
                return event_tx
                    .send(LiveSessionEvent::Error(error.to_string()))
                    .is_ok();
            }
        };

        let question = question.text;
        let context = answer_stream.context;
        let mut stream = answer_stream.stream;
        let mut answer = String::new();

        while let Some(delta) = stream.next().await {
            match delta {
                Ok(delta) => {
                    answer.push_str(&delta);
                    if event_tx.send(LiveSessionEvent::AnswerDelta(delta)).is_err() {
                        return false;
                    }
                }
                Err(error) => {
                    return event_tx
                        .send(LiveSessionEvent::Error(error.to_string()))
                        .is_ok();
                }
            }
        }

        if !answer.is_empty()
            && let Err(error) = self
                .persist_answer(
                    project_id,
                    session_id,
                    question,
                    Some(context),
                    answer.clone(),
                )
                .await
        {
            return event_tx
                .send(LiveSessionEvent::Error(error.to_string()))
                .is_ok();
        }

        event_tx
            .send(LiveSessionEvent::AnswerFinished(answer))
            .is_ok()
    }

    async fn flush_summary_window(
        &self,
        project_id: &str,
        session_id: &str,
        summarizer: &mut TranscriptSummaryAccumulator,
        event_tx: &Sender<LiveSessionEvent>,
    ) {
        let Some(window) = summarizer.take_window() else {
            return;
        };

        if let Err(error) = self
            .summarize_embed_and_store_window(project_id, session_id, window)
            .await
        {
            let _ = event_tx.send(LiveSessionEvent::Error(error.to_string()));
        }
    }

    /// Summarize one transcript window, persist it to SQLite, and index it in LanceDB.
    ///
    /// The resulting summary is part of retrieval context for later detected
    /// questions. It is embedded with both dense and sparse vectors so hybrid
    /// search can find summaries through semantic similarity, sparse keyword
    /// matching, and LanceDB FTS.
    async fn summarize_embed_and_store_window(
        &self,
        project_id: &str,
        session_id: &str,
        window: TranscriptSummaryWindow,
    ) -> Result<()> {
        let settings = settings_service::get_settings(&self.storage).await?;
        let api_key = llm_api_key()?;
        let llm = llm_from_settings(&settings, api_key);
        let summary = llm
            .summarize_transcript(&settings.llm_model, &window.transcript_text())
            .await?
            .content;

        if summary.trim().is_empty() {
            return Ok(());
        }

        let summary_index = next_summary_index(&self.storage, session_id).await?;
        let summary_id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now();

        transcript_summaries::ActiveModel {
            id: Set(summary_id.clone()),
            project_id: Set(project_id.to_string()),
            session_id: Set(session_id.to_string()),
            summary_index: Set(summary_index),
            chunk_start_index: Set(window.chunk_start_index),
            chunk_end_index: Set(window.chunk_end_index),
            start_ms: Set(window.start_ms),
            end_ms: Set(window.end_ms),
            duration_ms: Set(window.duration_ms()),
            summary: Set(summary.clone()),
            created_at: Set(now),
        }
        .insert(&self.storage.sqlite)
        .await?;

        let mut embedder = embedder_from_settings(&settings)?;
        let mut dense = embedder.dense_embed(vec![summary.clone()])?;
        let mut sparse = embedder.sparse_embed_vectors(vec![summary.clone()])?;
        let dense_vector = dense.pop().unwrap_or_default();
        let sparse_vector = sparse.pop().unwrap_or_default();

        self.storage
            .add_lance_transcript_summary(LanceTranscriptSummary {
                id: summary_id,
                project_id: project_id.to_string(),
                session_id: session_id.to_string(),
                summary_index,
                chunk_start_index: window.chunk_start_index,
                chunk_end_index: window.chunk_end_index,
                start_ms: window.start_ms,
                end_ms: window.end_ms,
                duration_ms: window.duration_ms(),
                summary,
                dense_vector,
                sparse_indices: sparse_vector.indices,
                sparse_values: sparse_vector.values,
                created_at_unix_secs: now.timestamp(),
            })
            .await?;

        Ok(())
    }

    async fn rebuild_search_data(
        &self,
        embedder: &mut Embedder,
        documents: Vec<documents::Model>,
    ) -> Result<()> {
        self.storage.rebuild_lance_schema().await?;

        for document in documents {
            upload::reindex_document(&self.storage, embedder, &document).await?;
        }

        self.rebuild_transcript_summary_search_data(embedder).await
    }

    async fn documents_to_rebuild(&self) -> Result<Vec<documents::Model>> {
        let documents = documents::Entity::find()
            .filter(documents::Column::Status.eq("ready"))
            .order_by_asc(documents::Column::CreatedAt)
            .all(&self.storage.sqlite)
            .await?;

        for document in &documents {
            upload::validate_document_reindex_source(document)?;
        }

        Ok(documents)
    }

    async fn rebuild_transcript_summary_search_data(&self, embedder: &mut Embedder) -> Result<()> {
        let summaries = transcript_summaries::Entity::find()
            .order_by_asc(transcript_summaries::Column::CreatedAt)
            .all(&self.storage.sqlite)
            .await?;
        for summary in summaries {
            let mut dense = embedder.dense_embed(vec![summary.summary.clone()])?;
            let mut sparse = embedder.sparse_embed_vectors(vec![summary.summary.clone()])?;
            let dense_vector = dense.pop().unwrap_or_default();
            let sparse_vector = sparse.pop().unwrap_or_default();

            self.storage
                .add_lance_transcript_summary(LanceTranscriptSummary {
                    id: summary.id,
                    project_id: summary.project_id,
                    session_id: summary.session_id,
                    summary_index: summary.summary_index,
                    chunk_start_index: summary.chunk_start_index,
                    chunk_end_index: summary.chunk_end_index,
                    start_ms: summary.start_ms,
                    end_ms: summary.end_ms,
                    duration_ms: summary.duration_ms,
                    summary: summary.summary,
                    dense_vector,
                    sparse_indices: sparse_vector.indices,
                    sparse_values: sparse_vector.values,
                    created_at_unix_secs: summary.created_at.timestamp(),
                })
                .await?;
        }

        Ok(())
    }

    /// Persist a completed suggested answer.
    pub async fn persist_answer(
        &self,
        project_id: &str,
        session_id: &str,
        question: String,
        context: Option<String>,
        answer: String,
    ) -> Result<()> {
        crate::answer::persist_answer(
            &self.storage,
            project_id,
            session_id,
            question,
            context,
            answer,
        )
        .await?;
        Ok(())
    }

    /// Lower-level retrieval entry point for workers/tests.
    pub async fn retrieve_project_context(
        &self,
        embedder: &mut Embedder,
        project_id: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<RetrievalHit>> {
        retrieval::retrieve_project_context(&self.storage, embedder, project_id, query, limit).await
    }

    /// Lower-level answer streaming entry point for callers with their own LLM.
    pub async fn stream_answer(
        &self,
        llm: &LlmClient,
        model: &str,
        question: &str,
        context: &str,
    ) -> Result<ChatStream> {
        crate::answer::stream_answer(llm, model, question, context).await
    }

    /// Drain a stream into storage and return the final answer text.
    ///
    /// This helper is useful for background jobs that do not need token-level UI
    /// updates. The live page should stream manually so users see first bytes as
    /// soon as they arrive.
    pub async fn persist_streamed_answer(
        &self,
        project_id: &str,
        session_id: &str,
        question: String,
        context: String,
        mut stream: ChatStream,
    ) -> Result<String> {
        let mut answer = String::new();
        while let Some(delta) = stream.next().await {
            answer.push_str(&delta?);
        }
        self.persist_answer(
            project_id,
            session_id,
            question,
            Some(context),
            answer.clone(),
        )
        .await?;
        Ok(answer)
    }

    /// Format retrieval hits into a prompt context string.
    pub fn context_text(&self, hits: &[RetrievalHit]) -> String {
        retrieval::format_context(hits)
    }
}

/// Build default settings form values used before storage finishes loading.
pub fn default_settings_form_view() -> SettingsFormView {
    SettingsFormView {
        llm_provider: "OpenAI-compatible".to_string(),
        llm_model: "gpt-4.1-mini".to_string(),
        llm_api_key_preview: String::new(),
        new_llm_api_key: String::new(),
        remove_llm_api_key: false,
        embedding_provider: "FastEmbed".to_string(),
        embedding_model: "jinaai/jina-embeddings-v2-base-code".to_string(),
        sparse_model: "prithivida/splade_pp_en_v1".to_string(),
        reranker_model: "jinaai/jina-reranker-v1-turbo-en".to_string(),
        whisper_model: stt::path::DEFAULT_WHISPER_MODEL_NAME.to_string(),
        summary_minutes: "5".to_string(),
        ui_theme: "light".to_string(),
    }
}

/// Convert persisted settings into an editable form model.
pub fn settings_form_view(settings: SettingsView) -> SettingsFormView {
    SettingsFormView {
        llm_provider: settings.llm_provider,
        llm_model: settings.llm_model,
        llm_api_key_preview: settings.llm_api_key_preview,
        new_llm_api_key: String::new(),
        remove_llm_api_key: false,
        embedding_provider: settings.embedding_provider,
        embedding_model: settings.embedding_model,
        sparse_model: settings.sparse_model,
        reranker_model: settings.reranker_model,
        whisper_model: settings.whisper_model,
        summary_minutes: settings.summary_minutes.to_string(),
        ui_theme: settings.ui_theme,
    }
}

/// Convert an editable form into the command used to persist settings.
pub fn settings_update_from_form_view(form: SettingsFormView) -> SettingsUpdate {
    SettingsUpdate {
        llm_model: form.llm_model,
        new_llm_api_key: optional_string(form.new_llm_api_key),
        remove_llm_api_key: form.remove_llm_api_key,
        embedding_model: form.embedding_model,
        sparse_model: form.sparse_model,
        reranker_model: form.reranker_model,
        whisper_model: form.whisper_model,
        summary_minutes: form.summary_minutes.parse::<i64>().unwrap_or(5),
        ui_theme: form.ui_theme,
    }
}

fn project_view(project: projects::Model) -> ProjectView {
    ProjectView {
        id: project.id,
        name: project.name,
        description: project.description.unwrap_or_default(),
    }
}

fn session_view(session: sessions::Model) -> SessionView {
    SessionView {
        id: session.id,
        title: session.title.unwrap_or_else(|| "Live session".to_string()),
        status: session.status.clone(),
        meta: format!(
            "{} • {}",
            session.started_at.format("%b %-d %H:%M"),
            session.status
        ),
    }
}

fn document_view(document: documents::Model) -> DocumentView {
    let kind = document
        .source_name
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_uppercase())
        .unwrap_or_else(|| "Text".to_string());

    DocumentView {
        id: document.id,
        name: document.source_name,
        meta: document
            .original_path
            .unwrap_or_else(|| "Path reference".to_string()),
        kind,
        status: document.status,
    }
}

fn settings_view(settings: settings::Model) -> SettingsView {
    let api_key = if settings.llm_api_key_ref.as_deref() == Some(LLM_API_KEY_REF) {
        llm_api_key().ok()
    } else {
        None
    };

    SettingsView {
        llm_provider: settings.llm_provider,
        llm_model: settings.llm_model,
        llm_api_key_preview: api_key.as_deref().map(mask_api_key).unwrap_or_default(),
        has_llm_api_key: api_key.is_some(),
        embedding_provider: settings.embedding_provider,
        embedding_model: settings.embedding_model,
        sparse_model: settings.sparse_model,
        reranker_model: settings.reranker_model,
        whisper_model: settings
            .whisper_model
            .unwrap_or_else(|| stt::path::DEFAULT_WHISPER_MODEL_NAME.to_string()),
        summary_minutes: settings.summary_minutes,
        ui_theme: settings.ui_theme,
    }
}

#[cfg(not(test))]
const LLM_API_KEY_SERVICE: &str = "tayori";
#[cfg(not(test))]
const LLM_API_KEY_ACCOUNT: &str = "llm-api-key";
const LLM_API_KEY_REF: &str = "os-keyring:tayori:llm-api-key";

#[cfg(not(test))]
fn llm_api_key_entry() -> Result<Entry> {
    Ok(Entry::new(LLM_API_KEY_SERVICE, LLM_API_KEY_ACCOUNT)?)
}

#[cfg(test)]
static TEST_LLM_API_KEY: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

#[cfg(not(test))]
fn llm_api_key() -> Result<String> {
    let key = llm_api_key_entry()?.get_password()?;
    if key.trim().is_empty() {
        anyhow::bail!("LLM API key is not configured");
    }
    Ok(key)
}

#[cfg(test)]
fn llm_api_key() -> Result<String> {
    let key = TEST_LLM_API_KEY
        .lock()
        .map_err(|_| anyhow::anyhow!("test keyring lock poisoned"))?
        .clone()
        .ok_or_else(|| anyhow::anyhow!("LLM API key is not configured"))?;
    if key.trim().is_empty() {
        anyhow::bail!("LLM API key is not configured");
    }
    Ok(key)
}

#[cfg(not(test))]
fn replace_llm_api_key(api_key: &str) -> Result<()> {
    let entry = llm_api_key_entry()?;
    entry.set_password(api_key)?;
    Ok(())
}

#[cfg(test)]
fn replace_llm_api_key(api_key: &str) -> Result<()> {
    *TEST_LLM_API_KEY
        .lock()
        .map_err(|_| anyhow::anyhow!("test keyring lock poisoned"))? = Some(api_key.to_string());
    Ok(())
}

#[cfg(not(test))]
fn delete_llm_api_key() -> Result<()> {
    match llm_api_key_entry()?.delete_credential() {
        Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
fn delete_llm_api_key() -> Result<()> {
    *TEST_LLM_API_KEY
        .lock()
        .map_err(|_| anyhow::anyhow!("test keyring lock poisoned"))? = None;
    Ok(())
}

fn mask_api_key(api_key: &str) -> String {
    let chars: Vec<_> = api_key.chars().collect();
    if chars.len() <= 6 {
        return "******".to_string();
    }

    let start: String = chars.iter().take(3).collect();
    let end: String = chars[chars.len() - 3..].iter().collect();
    format!("{start}******...{end}")
}

const WHISPER_MODEL_BASE_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";
const WHISPER_MODELS: &[WhisperModelDownload] = &[
    WhisperModelDownload {
        name: "tiny",
        sha1: "bd577a113a864445d4c299885e0cb97d4ba92b5f",
    },
    WhisperModelDownload {
        name: "tiny-q5_1",
        sha1: "2827a03e495b1ed3048ef28a6a4620537db4ee51",
    },
    WhisperModelDownload {
        name: "tiny-q8_0",
        sha1: "19e8118f6652a650569f5a949d962154e01571d9",
    },
    WhisperModelDownload {
        name: "tiny.en",
        sha1: "c78c86eb1a8faa21b369bcd33207cc90d64ae9df",
    },
    WhisperModelDownload {
        name: "tiny.en-q5_1",
        sha1: "3fb92ec865cbbc769f08137f22470d6b66e071b6",
    },
    WhisperModelDownload {
        name: "tiny.en-q8_0",
        sha1: "802d6668e7d411123e672abe4cb6c18f12306abb",
    },
    WhisperModelDownload {
        name: "base",
        sha1: "465707469ff3a37a2b9b8d8f89f2f99de7299dac",
    },
    WhisperModelDownload {
        name: "base-q5_1",
        sha1: "a3733eda680ef76256db5fc5dd9de8629e62c5e7",
    },
    WhisperModelDownload {
        name: "base-q8_0",
        sha1: "7bb89bb49ed6955013b166f1b6a6c04584a20fbe",
    },
    WhisperModelDownload {
        name: "base.en",
        sha1: "137c40403d78fd54d454da0f9bd998f78703390c",
    },
    WhisperModelDownload {
        name: "base.en-q5_1",
        sha1: "d26d7ce5a1b6e57bea5d0431b9c20ae49423c94a",
    },
    WhisperModelDownload {
        name: "base.en-q8_0",
        sha1: "bb1574182e9b924452bf0cd1510ac034d323e948",
    },
    WhisperModelDownload {
        name: "small",
        sha1: "55356645c2b361a969dfd0ef2c5a50d530afd8d5",
    },
    WhisperModelDownload {
        name: "small-q5_1",
        sha1: "6fe57ddcfdd1c6b07cdcc73aaf620810ce5fc771",
    },
    WhisperModelDownload {
        name: "small-q8_0",
        sha1: "bcad8a2083f4e53d648d586b7dbc0cd673d8afad",
    },
    WhisperModelDownload {
        name: "small.en",
        sha1: "db8a495a91d927739e50b3fc1cc4c6b8f6c2d022",
    },
    WhisperModelDownload {
        name: "small.en-q5_1",
        sha1: "20f54878d608f94e4a8ee3ae56016571d47cba34",
    },
    WhisperModelDownload {
        name: "small.en-q8_0",
        sha1: "9d75ff4ccfa0a8217870d7405cf8cef0a5579852",
    },
    WhisperModelDownload {
        name: "small.en-tdrz",
        sha1: "b6c6e7e89af1a35c08e6de56b66ca6a02a2fdfa1",
    },
    WhisperModelDownload {
        name: "medium",
        sha1: "fd9727b6e1217c2f614f9b698455c4ffd82463b4",
    },
    WhisperModelDownload {
        name: "medium-q5_0",
        sha1: "7718d4c1ec62ca96998f058114db98236937490e",
    },
    WhisperModelDownload {
        name: "medium-q8_0",
        sha1: "e66645948aff4bebbec71b3485c576f3d63af5d6",
    },
    WhisperModelDownload {
        name: "medium.en",
        sha1: "8c30f0e44ce9560643ebd10bbe50cd20eafd3723",
    },
    WhisperModelDownload {
        name: "medium.en-q5_0",
        sha1: "bb3b5281bddd61605d6fc76bc5b92d8f20284c3b",
    },
    WhisperModelDownload {
        name: "medium.en-q8_0",
        sha1: "b1cf48c12c807e14881f634fb7b6c6ca867f6b38",
    },
    WhisperModelDownload {
        name: "large-v1",
        sha1: "b1caaf735c4cc1429223d5a74f0f4d0b9b59a299",
    },
    WhisperModelDownload {
        name: "large-v2",
        sha1: "0f4c8e34f21cf1a914c59d8b3ce882345ad349d6",
    },
    WhisperModelDownload {
        name: "large-v2-q5_0",
        sha1: "00e39f2196344e901b3a2bd5814807a769bd1630",
    },
    WhisperModelDownload {
        name: "large-v2-q8_0",
        sha1: "da97d6ca8f8ffbeeb5fd147f79010eeea194ba38",
    },
    WhisperModelDownload {
        name: "large-v3",
        sha1: "ad82bf6a9043ceed055076d0fd39f5f186ff8062",
    },
    WhisperModelDownload {
        name: "large-v3-q5_0",
        sha1: "e6e2ed78495d403bef4b7cff42ef4aaadcfea8de",
    },
    WhisperModelDownload {
        name: "large-v3-turbo",
        sha1: "4af2b29d7ec73d781377bfd1758ca957a807e941",
    },
    WhisperModelDownload {
        name: "large-v3-turbo-q5_0",
        sha1: "e050f7970618a659205450ad97eb95a18d69c9ee",
    },
    WhisperModelDownload {
        name: "large-v3-turbo-q8_0",
        sha1: "01bf15bedffe9f39d65c1b6ff9b687ea91f59e0e",
    },
];

struct WhisperModelDownload {
    name: &'static str,
    sha1: &'static str,
}

fn install_whisper_model(model_name: &str) -> Result<PathBuf> {
    let model = whisper_model_download(model_name)?;
    let path = stt::path::model_path(model.name);
    stt::path::ensure_path_exists(&path)?;

    if path.is_file() {
        if sha1_hex(&path)? == model.sha1 {
            return Ok(path);
        }
        std::fs::remove_file(&path)?;
    }

    let part_path = path.with_extension("bin.part");
    download_whisper_model_part(model.name, &part_path)?;

    let actual_sha1 = sha1_hex(&part_path)?;
    if actual_sha1 != model.sha1 {
        let _ = std::fs::remove_file(&part_path);
        anyhow::bail!(
            "Whisper model checksum mismatch for {model_name}; expected {}, got {}. Retry the download.",
            model.sha1,
            actual_sha1
        );
    }

    std::fs::rename(&part_path, &path)?;
    Ok(path)
}

fn download_whisper_model_part(model_name: &str, part_path: &std::path::Path) -> Result<()> {
    let filename = format!("ggml-{model_name}.bin");
    let url = format!("{WHISPER_MODEL_BASE_URL}/{filename}");
    let existing_len = part_path
        .metadata()
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    let client = Client::new();
    let mut request = client.get(url);
    if existing_len > 0 {
        request = request.header(RANGE, format!("bytes={existing_len}-"));
    }

    let mut response = request.send()?;
    let status = response.status();
    let append = existing_len > 0 && status == StatusCode::PARTIAL_CONTENT;
    if !(status.is_success() || status == StatusCode::PARTIAL_CONTENT) {
        anyhow::bail!("failed to download Whisper model {model_name}: HTTP {status}");
    }

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(append)
        .truncate(!append)
        .open(part_path)?;
    std::io::copy(&mut response, &mut file)?;
    Ok(())
}

fn whisper_model_download(model_name: &str) -> Result<&'static WhisperModelDownload> {
    WHISPER_MODELS
        .iter()
        .find(|model| model.name == model_name)
        .ok_or_else(|| anyhow::anyhow!("unknown Whisper model name: {model_name}"))
}

fn sha1_hex(path: &std::path::Path) -> Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha1::new();
    std::io::copy(&mut file, &mut hasher)?;
    Ok(format!("{:x}", hasher.finalize()))
}

fn whisper_model_view(model_name: &str) -> Result<WhisperModelView> {
    let model_name = optional_string(model_name.to_string())
        .unwrap_or_else(|| stt::path::DEFAULT_WHISPER_MODEL_NAME.to_string());
    validate_whisper_model_name(&model_name)?;
    let path = stt::path::model_path(&model_name);
    Ok(WhisperModelView {
        model_name,
        installed: path.is_file(),
        path: path.to_string_lossy().to_string(),
    })
}

fn validate_whisper_model_name_or_blank(model_name: &str) -> Result<()> {
    if model_name.trim().is_empty() {
        return Ok(());
    }
    validate_whisper_model_name(model_name)
}

fn validate_whisper_model_name(model_name: &str) -> Result<()> {
    let valid = !model_name.trim().is_empty()
        && model_name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
        && !model_name.contains("..");
    anyhow::ensure!(valid, "invalid Whisper model name: {model_name}");
    Ok(())
}

fn transcript_chunk_view(
    chunk: transcript_chunks::Model,
    detection: detection::Detection,
) -> TranscriptChunkView {
    TranscriptChunkView {
        id: chunk.id,
        text: chunk.text,
        time: format_duration(chunk.start_ms),
        chunk_index: chunk.chunk_index,
        start_ms: chunk.start_ms,
        end_ms: chunk.end_ms,
        duration_ms: chunk.duration_ms,
        has_question: detection.has_question,
        confidence: (detection.confidence.clamp(0.0, 1.0) * 100.0).round() as u8,
    }
}

#[derive(Debug)]
struct TranscriptSummaryWindow {
    chunk_start_index: i64,
    chunk_end_index: i64,
    start_ms: i64,
    end_ms: i64,
    chunks: Vec<TranscriptChunkView>,
}

impl TranscriptSummaryWindow {
    fn duration_ms(&self) -> i64 {
        self.end_ms.saturating_sub(self.start_ms)
    }

    fn transcript_text(&self) -> String {
        self.chunks
            .iter()
            .map(|chunk| format!("[{}] {}", chunk.time, chunk.text))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[derive(Debug)]
struct TranscriptSummaryAccumulator {
    window_ms: i64,
    chunks: Vec<TranscriptChunkView>,
}

impl TranscriptSummaryAccumulator {
    fn new(summary_minutes: i64) -> Self {
        Self {
            window_ms: summary_minutes.clamp(1, 10) * 60_000,
            chunks: Vec::new(),
        }
    }

    fn push(&mut self, chunk: TranscriptChunkView) -> bool {
        self.chunks.push(chunk);
        self.ready()
    }

    fn ready(&self) -> bool {
        let Some(first) = self.chunks.first() else {
            return false;
        };
        let Some(last) = self.chunks.last() else {
            return false;
        };
        last.end_ms.saturating_sub(first.start_ms) >= self.window_ms
    }

    fn take_window(&mut self) -> Option<TranscriptSummaryWindow> {
        if self.chunks.is_empty() {
            return None;
        }

        let chunks = std::mem::take(&mut self.chunks);
        let first = chunks.first()?;
        let last = chunks.last()?;
        Some(TranscriptSummaryWindow {
            chunk_start_index: first.chunk_index,
            chunk_end_index: last.chunk_index,
            start_ms: first.start_ms,
            end_ms: last.end_ms,
            chunks,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DetectedQuestionWindow {
    text: String,
    confidence: u8,
}

#[derive(Debug, Default)]
struct RollingQuestionDetector {
    chunks: Vec<TranscriptChunkView>,
    pending: Option<DetectedQuestionWindow>,
    answered_end_index: Option<i64>,
}

impl RollingQuestionDetector {
    fn new() -> Self {
        Self::default()
    }

    fn push(&mut self, chunk: TranscriptChunkView) -> Option<DetectedQuestionWindow> {
        self.chunks.push(chunk);
        if self.chunks.len() > 3 {
            self.chunks.remove(0);
        }

        if let Some(pending) = self.pending.take() {
            self.answered_end_index = self.chunks.last().map(|chunk| chunk.chunk_index);
            return Some(DetectedQuestionWindow {
                text: self.question_text(),
                confidence: pending.confidence,
            });
        }

        let latest = self.chunks.last()?;
        if self
            .answered_end_index
            .is_some_and(|index| latest.chunk_index <= index)
        {
            return None;
        }

        let text = self.question_text();
        let detection = detection::detect_question_or_command(&text);
        if !detection.has_question {
            return None;
        }

        let question = DetectedQuestionWindow {
            text,
            confidence: (detection.confidence.clamp(0.0, 1.0) * 100.0).round() as u8,
        };

        if latest_looks_complete(latest) {
            self.answered_end_index = Some(latest.chunk_index);
            Some(question)
        } else {
            self.pending = Some(question);
            None
        }
    }

    fn question_text(&self) -> String {
        self.chunks
            .iter()
            .map(|chunk| chunk.text.trim())
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

fn latest_looks_complete(chunk: &TranscriptChunkView) -> bool {
    chunk.text.trim_end().ends_with(['?', '.', '!', ':'])
}

async fn next_summary_index(storage: &Storage, session_id: &str) -> Result<i64> {
    let latest = transcript_summaries::Entity::find()
        .filter(transcript_summaries::Column::SessionId.eq(session_id))
        .order_by_desc(transcript_summaries::Column::SummaryIndex)
        .one(&storage.sqlite)
        .await?;

    Ok(latest.map(|summary| summary.summary_index + 1).unwrap_or(0))
}

fn format_duration(ms: i64) -> String {
    let total_seconds = ms.max(0) / 1000;
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    format!("{minutes:02}:{seconds:02}")
}

fn optional_string(value: String) -> Option<String> {
    let value = value.trim().to_string();
    if value.is_empty() { None } else { Some(value) }
}

pub fn embedder_from_settings(settings: &settings::Model) -> Result<Embedder> {
    Embedder::new(
        &settings.embedding_model,
        &settings.sparse_model,
        &settings.reranker_model,
    )
}

pub fn llm_from_settings(settings: &settings::Model, api_key: impl Into<String>) -> LlmClient {
    match settings.llm_provider.as_str() {
        "openai" => LlmClient::new(api_key),
        _ => LlmClient::new(api_key),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use detection::Detection;

    fn now() -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 5, 14, 10, 30, 0)
            .single()
            .unwrap()
    }

    async fn test_core() -> TayoriCore {
        let temp = tempfile::tempdir().unwrap().keep();
        let sqlite_path = temp.join("tayori.db");
        let sqlite = Database::connect(format!("sqlite://{}?mode=rwc", sqlite_path.display()))
            .await
            .unwrap();
        Migrator::up(&sqlite, None).await.unwrap();
        let lancedb = lancedb::connect("memory://").execute().await.unwrap();

        TayoriCore {
            storage: Arc::new(Storage { sqlite, lancedb }),
        }
    }

    #[test]
    fn optional_string_trims_empty_values() {
        assert_eq!(optional_string("  ".to_string()), None);
        assert_eq!(
            optional_string("  value  ".to_string()),
            Some("value".to_string())
        );
    }

    #[test]
    fn api_key_mask_keeps_only_first_and_last_three_chars() {
        assert_eq!(mask_api_key("sk-123456789abc"), "sk-******...abc");
        assert_eq!(mask_api_key("short"), "******");
    }

    #[test]
    #[ignore = "uses the host OS keyring"]
    fn os_keyring_roundtrip_reads_written_secret() {
        let account = uuid::Uuid::new_v4().to_string();
        let entry = Entry::new("tayori-test", &account).unwrap();

        entry.set_password("sk-test-key").unwrap();
        assert_eq!(entry.get_password().unwrap(), "sk-test-key");
        let _ = entry.delete_credential();
    }

    #[tokio::test]
    async fn api_key_save_writes_keyring_and_sqlite_marker() {
        delete_llm_api_key().unwrap();
        let core = test_core().await;
        let mut form = core.settings_form().await.unwrap();
        form.new_llm_api_key = "sk-test-123456789abc".to_string();

        let saved = core.update_settings_form(form).await.unwrap();
        let model = settings_service::get_settings(&core.storage).await.unwrap();

        assert_eq!(llm_api_key().unwrap(), "sk-test-123456789abc");
        assert_eq!(model.llm_api_key_ref.as_deref(), Some(LLM_API_KEY_REF));
        assert_eq!(saved.llm_api_key_preview, "sk-******...abc");
    }

    #[tokio::test]
    async fn settings_reload_reads_sqlite_marker_and_keyring_preview() {
        delete_llm_api_key().unwrap();
        replace_llm_api_key("sk-reload-123456abc").unwrap();
        let core = test_core().await;
        let mut model = settings_service::get_settings(&core.storage).await.unwrap();
        model.llm_api_key_ref = Some(LLM_API_KEY_REF.to_string());
        settings_service::update_settings(&core.storage, model)
            .await
            .unwrap();

        let reloaded = core.settings_form().await.unwrap();

        assert_eq!(reloaded.llm_api_key_preview, "sk-******...abc");
        assert!(reloaded.new_llm_api_key.is_empty());
        assert!(!reloaded.remove_llm_api_key);
    }

    #[test]
    fn duration_format_clamps_negative_values_and_formats_minutes() {
        assert_eq!(format_duration(-100), "00:00");
        assert_eq!(format_duration(65_432), "01:05");
        assert_eq!(format_duration(3_601_000), "60:01");
    }

    #[test]
    fn project_view_normalizes_missing_description() {
        let view = project_view(projects::Model {
            id: "project-1".to_string(),
            name: "Roadmap".to_string(),
            description: None,
            created_at: now(),
            updated_at: now(),
        });

        assert_eq!(view.description, "");
    }

    #[test]
    fn document_view_derives_kind_from_extension_and_preserves_status() {
        let view = document_view(documents::Model {
            id: "document-1".to_string(),
            project_id: "project-1".to_string(),
            source_name: "notes.md".to_string(),
            original_path: Some("/tmp/notes.md".to_string()),
            content_hash: Some("hash".to_string()),
            status: "ready".to_string(),
            error_message: None,
            created_at: now(),
            updated_at: now(),
        });

        assert_eq!(view.kind, "MD");
        assert_eq!(view.meta, "/tmp/notes.md");
        assert_eq!(view.status, "ready");
    }

    #[test]
    fn session_view_uses_default_title_when_missing() {
        let view = session_view(sessions::Model {
            id: "session-1".to_string(),
            project_id: "project-1".to_string(),
            title: None,
            status: "running".to_string(),
            started_at: now(),
            ended_at: None,
            created_at: now(),
            updated_at: now(),
        });

        assert_eq!(view.title, "Live session");
        assert!(view.meta.contains("running"));
    }

    #[test]
    fn settings_view_uses_default_whisper_model_when_database_value_is_missing() {
        let view = settings_view(settings::Model {
            id: "default".to_string(),
            llm_provider: "openai".to_string(),
            llm_model: "gpt-4.1-mini".to_string(),
            llm_api_key_ref: None,
            llm_store_responses: 1,
            embedding_provider: "fastembed".to_string(),
            embedding_model: "dense".to_string(),
            sparse_model: "sparse".to_string(),
            embedding_dimension: 768,
            reranker_model: "reranker".to_string(),
            whisper_model: None,
            summary_minutes: 5,
            ui_theme: "light".to_string(),
            created_at: now(),
            updated_at: now(),
        });

        assert_eq!(view.whisper_model, stt::path::DEFAULT_WHISPER_MODEL_NAME);
        assert_eq!(view.llm_api_key_preview, "");
    }

    #[test]
    fn settings_view_ignores_keyring_without_sqlite_marker() {
        let view = settings_view(settings::Model {
            id: "default".to_string(),
            llm_provider: "openai".to_string(),
            llm_model: "gpt-4.1-mini".to_string(),
            llm_api_key_ref: None,
            llm_store_responses: 1,
            embedding_provider: "fastembed".to_string(),
            embedding_model: "dense".to_string(),
            sparse_model: "sparse".to_string(),
            embedding_dimension: 768,
            reranker_model: "reranker".to_string(),
            whisper_model: Some("small-q8_0".to_string()),
            summary_minutes: 5,
            ui_theme: "light".to_string(),
            created_at: now(),
            updated_at: now(),
        });

        assert!(!view.has_llm_api_key);
        assert_eq!(view.llm_api_key_preview, "");
    }

    #[test]
    fn transcript_chunk_view_maps_detection_to_percent_confidence() {
        let view = transcript_chunk_view(
            transcript_chunks::Model {
                id: "chunk-1".to_string(),
                session_id: "session-1".to_string(),
                project_id: "project-1".to_string(),
                chunk_index: 7,
                text: "Can we ship this?".to_string(),
                start_ms: 65_000,
                end_ms: 68_000,
                duration_ms: 3_000,
                created_at: now(),
            },
            Detection {
                has_question: true,
                has_command: false,
                confidence: 0.914,
                reason: "question-mark".to_string(),
            },
        );

        assert!(view.has_question);
        assert_eq!(view.confidence, 91);
        assert_eq!(view.time, "01:05");
    }

    #[test]
    fn summary_accumulator_flushes_after_configured_window() {
        let mut accumulator = TranscriptSummaryAccumulator::new(1);
        let first = TranscriptChunkView {
            id: "first".to_string(),
            text: "kickoff notes".to_string(),
            time: "00:00".to_string(),
            chunk_index: 0,
            start_ms: 0,
            end_ms: 30_000,
            duration_ms: 30_000,
            has_question: false,
            confidence: 0,
        };
        let second = TranscriptChunkView {
            id: "second".to_string(),
            text: "decision made".to_string(),
            time: "01:00".to_string(),
            chunk_index: 1,
            start_ms: 60_000,
            end_ms: 65_000,
            duration_ms: 5_000,
            has_question: false,
            confidence: 0,
        };

        assert!(!accumulator.push(first));
        assert!(accumulator.push(second));

        let window = accumulator.take_window().unwrap();
        assert_eq!(window.chunk_start_index, 0);
        assert_eq!(window.chunk_end_index, 1);
        assert_eq!(window.duration_ms(), 65_000);
        assert!(window.transcript_text().contains("kickoff notes"));
        assert!(accumulator.take_window().is_none());
    }

    #[test]
    fn whisper_model_catalog_contains_default_model() {
        let model = whisper_model_download(stt::path::DEFAULT_WHISPER_MODEL_NAME).unwrap();

        assert_eq!(model.name, "small-q8_0");
        assert_eq!(model.sha1, "bcad8a2083f4e53d648d586b7dbc0cd673d8afad");
    }

    #[test]
    fn whisper_model_view_uses_default_for_blank_name() {
        let view = whisper_model_view("  ").unwrap();

        assert_eq!(view.model_name, stt::path::DEFAULT_WHISPER_MODEL_NAME);
        assert!(view.path.ends_with("ggml-small-q8_0.bin"));
    }

    #[test]
    fn whisper_model_names_reject_path_traversal() {
        assert!(validate_whisper_model_name("small-q8_0").is_ok());
        assert!(validate_whisper_model_name("../secret").is_err());
        assert!(validate_whisper_model_name("small/q8_0").is_err());
    }

    #[test]
    fn settings_form_update_defaults_blank_whisper_to_default_model() {
        let update = settings_update_from_form_view(SettingsFormView {
            whisper_model: "  ".to_string(),
            ..default_settings_form_view()
        });

        assert_eq!(update.whisper_model.trim(), "");
    }
}
