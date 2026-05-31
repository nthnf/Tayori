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

use sea_orm::EntityTrait;
use tayori::backend::entities::settings;
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
                    let _ = install::install_moonshine(&transcript_model, None).await;
                    ds.with_mut(|s| s.retain(|x| !x.starts_with("Moonshine")));
                }

                let silero_path = install::default_silero_path(None);
                if !silero_path.exists() {
                    ds.with_mut(|s| s.push("Silero VAD".to_string()));
                    let _ = install::install_silero(None).await;
                    ds.with_mut(|s| s.retain(|x| x != "Silero VAD"));
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

    // 5. Eagerly initialize the embedder and detector in the background
    //    so they're ready before the user starts a live session.
    use_future(move || {
        let state = state.clone();
        async move {
            let embedder_cell = state.embedder.clone();
            let detector_cell = state.detector.clone();
            let _ = tokio::task::spawn_blocking(move || {
                if embedder_cell.get().is_none() {
                    match tayori::backend::models::embed::Embedder::new() {
                        Ok(e) => {
                            let _ = embedder_cell.set(e);
                        }
                        Err(e) => tracing::error!("Failed to init embedder at startup: {e}"),
                    }
                }
                if detector_cell.get().is_none() {
                    match tayori::backend::detection::IntentDetector::new() {
                        Ok(d) => {
                            let _ = detector_cell.set(d);
                        }
                        Err(e) => tracing::error!("Failed to init detector at startup: {e}"),
                    }
                }
            })
            .await;
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
