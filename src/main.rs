use dioxus::prelude::*;
use migration::{Migrator, MigratorTrait};
use tayori::Route;
use tayori::backend::db::{connect, default_sqlite_uri, init_vector_indexes};

const FAVICON: Asset = asset!("/assets/favicon.ico");
const MAIN_CSS: Asset = asset!("/assets/main.css");
const TAILWIND_CSS: Asset = asset!("/assets/tailwind.css");

fn main() {
    dioxus::launch(App);
}

use chrono::Utc;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use tayori::backend::entities::{sessions, settings};
use tayori::backend::models::install;

#[component]
fn App() -> Element {
    // 1. Hold the database connection locally in a signal
    let mut db_conn = use_signal(|| None::<sea_orm::DatabaseConnection>);
    let mut error_msg = use_signal(|| None::<String>);
    let download_status = use_signal(Vec::<String>::new);

    // 2. Initialize and migrate the database async on boot
    use_future(move || async move {
        match async {
            let db_url = default_sqlite_uri()?;
            let db = connect(&db_url).await?;
            Migrator::up(&db, None).await?;
            init_vector_indexes(&db).await?;

            // 3. Cleanup hanging sessions from previous runs (e.g. app closed forcefully)
            let hanging_sessions = sessions::Entity::find()
                .filter(sessions::Column::EndedAt.is_null())
                .all(&db)
                .await?;
            for session in hanging_sessions {
                let mut active_session: sessions::ActiveModel = session.into();
                active_session.ended_at = Set(Some(Utc::now()));
                active_session.status = Set("cancelled".to_string());
                active_session.updated_at = Set(Utc::now());
                if let Err(e) = active_session.update(&db).await {
                    tracing::error!("Failed to cleanup hanging session: {}", e);
                }
            }

            // Fetch transcript model setting or use default
            let transcript_model = match settings::Entity::find_by_id("default").one(&db).await? {
                Some(s) => s.transcript_model,
                None => "base".to_string(), // fallback
            };

            // Spawn background task for downloads so we don't block app load
            let mut ds = download_status;
            spawn(async move {
                let moonshine_path = install::moonshine_path(&transcript_model, None);
                if !moonshine_path.exists() {
                    ds.with_mut(|s| s.push(format!("Moonshine STT ({})", transcript_model)));
                    if let Err(e) = install::install_moonshine(&transcript_model, None).await {
                        tracing::error!("Failed to install moonshine model: {}", e);
                    }
                    ds.with_mut(|s| s.retain(|x| !x.starts_with("Moonshine")));
                }

                let silero_path = install::default_silero_path(None);
                if !silero_path.exists() {
                    ds.with_mut(|s| s.push("Silero VAD".to_string()));
                    if let Err(e) = install::install_silero(None).await {
                        tracing::error!("Failed to install silero model: {}", e);
                    }
                    ds.with_mut(|s| s.retain(|x| x != "Silero VAD"));
                }

                let pos_model_path = install::default_pos_model_path(None);
                let pos_tokenizer_path = install::default_pos_tokenizer_path(None);
                if !pos_model_path.exists() || !pos_tokenizer_path.exists() {
                    ds.with_mut(|s| s.push("MobileBERT POS".to_string()));
                    if let Err(e) = install::install_pos(None).await {
                        tracing::error!("Failed to install POS model: {}", e);
                    }
                    ds.with_mut(|s| s.retain(|x| x != "MobileBERT POS"));
                }
            });

            Ok::<_, anyhow::Error>(db)
        }
        .await
        {
            Ok(db) => db_conn.set(Some(db)),
            Err(e) => error_msg.set(Some(e.to_string())),
        }
    });

    // 3. Show error or loading screen until ready
    if let Some(err) = error_msg() {
        return rsx! {
            div { "Critical Error: Failed to initialize database: {err}" }
        };
    }

    let Some(db) = db_conn() else {
        return rsx! {
            div { class: "p-4 text-center text-sm text-muted",
                "Loading database and running migrations..."
            }
        };
    };

    // 4. Inject the connected DB into AppState for all pages
    let state = use_context_provider(move || tayori::state::AppState::new(db.clone()));

    // 5. Eagerly initialize the embedder, detector, and graph pipeline in the background
    //    so they're ready before the user starts a live session.
    use_future(move || {
        let state = state.clone();
        async move {
            let embedder_cell = state.embedder.clone();
            let detector_cell = state.detector.clone();
            let pos_model_cell = state.pos_model.clone();
            if let Err(e) = tokio::task::spawn_blocking(move || {
                if embedder_cell.get().is_none() {
                    match tayori::backend::models::embed::Embedder::new() {
                        Ok(e) => {
                            if embedder_cell.set(e).is_err() {
                                tracing::warn!("Embedder was already initialized.");
                            }
                        }
                        Err(e) => tracing::error!("Failed to init embedder at startup: {e}"),
                    }
                }
                if detector_cell.get().is_none() {
                    match tayori::backend::detection::IntentDetector::new() {
                        Ok(d) => {
                            if detector_cell.set(d).is_err() {
                                tracing::warn!("Detector was already initialized.");
                            }
                        }
                        Err(e) => tracing::error!("Failed to init detector at startup: {e}"),
                    }
                }
                if pos_model_cell.get().is_none() {
                    let model_path = install::default_pos_model_path(None);
                    let tokenizer_path = install::default_pos_tokenizer_path(None);
                    if model_path.exists() && tokenizer_path.exists() {
                        match tayori::backend::models::pos::PosModel::new(
                            &model_path,
                            &tokenizer_path,
                        ) {
                            Ok(gp) => {
                                if pos_model_cell.set(gp).is_err() {
                                    tracing::warn!("POS model was already initialized.");
                                }
                            }
                            Err(e) => {
                                tracing::error!("Failed to init pos model at startup: {e}")
                            }
                        }
                    }
                }
            })
            .await
            {
                tracing::error!("Background initialization task panicked: {}", e);
            }
        }
    });

    rsx! {
        document::Link { rel: "icon", href: FAVICON }
        document::Link { rel: "stylesheet", href: MAIN_CSS }
        document::Link { rel: "stylesheet", href: TAILWIND_CSS }

        Router::<Route> {}

        if !download_status().is_empty() {
            div { class: "fixed bottom-0 left-0 right-0 z-50 flex items-center justify-between border-t border-hairline bg-surface-dark px-4 py-3 text-sm font-semibold text-on-dark shadow-lg",
                div { class: "flex items-center gap-3",
                    svg {
                        class: "h-4 w-4 animate-spin text-primary",
                        xmlns: "http://www.w3.org/2000/svg",
                        fill: "none",
                        view_box: "0 0 24 24",
                        circle {
                            class: "opacity-25",
                            cx: "12",
                            cy: "12",
                            r: "10",
                            stroke: "currentColor",
                            stroke_width: "4",
                        }
                        path {
                            class: "opacity-75",
                            fill: "currentColor",
                            d: "M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z",
                        }
                    }
                    span { "Downloading models: {download_status().join(\", \")}..." }
                }
            }
        }
    }
}
