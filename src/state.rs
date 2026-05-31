use crate::backend::audio::runtime::AudioRuntime;
use crate::backend::detection::IntentDetector;
use crate::backend::models::embed::Embedder;
use crate::backend::models::llm::LlmModel;
use sea_orm::DatabaseConnection;
use std::sync::Arc;
use tokio::sync::{Mutex, OnceCell};

/// Global application state injected at the root of the Dioxus app.
/// This prevents expensive resources (like DB connections and audio runtimes)
/// from being dropped when the user navigates between pages.
#[derive(Clone)]
pub struct AppState {
    pub db: DatabaseConnection,

    // Wrapped in Arc/Mutex so they can be mutated safely from any page
    // and are initialized lazily (Options) so we don't block app startup.
    pub audio_runtime: Arc<Mutex<Option<AudioRuntime>>>,
    pub llm: Arc<Mutex<Option<LlmModel>>>,
    // Stateless once initialized, so OnceCell is perfect
    pub embedder: Arc<OnceCell<Embedder>>,
    pub detector: Arc<OnceCell<IntentDetector>>,
}

impl AppState {
    pub fn new(db: DatabaseConnection) -> Self {
        Self {
            db,
            audio_runtime: Arc::new(Mutex::new(None)),
            llm: Arc::new(Mutex::new(None)),
            embedder: Arc::new(OnceCell::new()),
            detector: Arc::new(OnceCell::new()),
        }
    }
}
