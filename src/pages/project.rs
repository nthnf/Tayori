use crate::Route;
use crate::backend::pages::project::ProjectPageModel;
use crate::components::error::ErrorView;
use crate::state::AppState;
use dioxus::prelude::*;

#[component]
pub fn ProjectPage(id: String) -> Element {
    let state = use_context::<AppState>();
    let navigator = use_navigator();
    let mut action_error = use_signal(|| None::<String>);

    let state_for_load = state.clone();
    let id_for_load = id.clone();
    let mut data = use_resource(move || {
        let model = ProjectPageModel::new(state_for_load.clone());
        let pid = id_for_load.clone();
        async move { model.load(pid).await.map_err(|e| format!("{e:#}")) }
    });

    let state_for_upload = state.clone();
    let id_for_upload = id.clone();
    let on_upload = move |_| {
        let state_for_upload = state_for_upload.clone();
        let pid = id_for_upload.clone();
        spawn(async move {
            let Some(file_handle) = rfd::AsyncFileDialog::new()
                .add_filter("Text documents", &["txt", "md", "markdown", "csv", "json"])
                .pick_file()
                .await
            else {
                return;
            };

            let bytes = file_handle.read().await;
            let file_name = file_handle.file_name();
            let path_std = std::path::Path::new(&file_name);
            let ext = path_std
                .extension()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();

            let model = ProjectPageModel::new(state_for_upload.clone());
            match model.create_ingesting_doc(pid, file_name).await {
                Ok(doc_id) => {
                    data.restart(); // Show ingesting status immediately

                    if let Err(e) = model.process_and_ready_doc(doc_id, &bytes, &ext).await {
                        tracing::error!("Failed to process document: {}", e);
                        action_error.set(Some(e.to_string()));
                    } else {
                        action_error.set(None);
                    }
                    data.restart(); // Show ready/failed status
                }
                Err(e) => {
                    tracing::error!("Failed to create document: {}", e);
                    action_error.set(Some(e.to_string()));
                    data.restart();
                }
            }
        });
    };

    let state_for_create = state.clone();
    let id_for_create = id.clone();
    let on_create_session = move |_| {
        let model = ProjectPageModel::new(state_for_create.clone());
        let pid = id_for_create.clone();
        spawn(async move {
            match model
                .create_session(pid.clone(), Some("New Recording".to_string()))
                .await
            {
                Ok(session_id) => {
                    action_error.set(None);
                    data.restart();
                    navigator.push(Route::LivePage {
                        project_id: pid,
                        session_id,
                    });
                }
                Err(e) => {
                    tracing::error!("Failed to create session: {}", e);
                    action_error.set(Some(e.to_string()));
                }
            }
        });
    };

    let project_name = "Workspace".to_string(); // In a real app, you'd fetch the project name too.

    rsx! {
        div { class: "grid h-full min-h-0 gap-4 lg:grid-cols-[minmax(0,1fr)_340px]",
            section { class: "flex min-h-0 flex-col overflow-hidden rounded-lg border border-hairline bg-canvas shadow-sm",
                div { class: "flex items-center justify-between border-b border-hairline bg-surface-soft px-4 py-3",
                    div {
                        h2 { class: "text-lg font-semibold text-ink", "{project_name}" }
                        p { class: "text-sm text-body", "Documents and metadata" }
                    }
                }

                if let Some(err) = action_error() {
                    div { class: "px-4 pt-4",
                        ErrorView {
                            message: err,
                            on_retry: move |_| action_error.set(None),
                        }
                    }
                }

                div { class: "grid min-h-0 flex-1 grid-rows-[210px_minmax(0,1fr)] gap-4 p-4",
                    button {
                        class: "grid place-items-center rounded-lg border border-dashed border-hairline bg-surface-soft p-6 text-center hover:border-primary hover:bg-canvas",
                        onclick: on_upload,
                        div {
                            div { class: "mx-auto mb-4 grid h-12 w-12 place-items-center rounded-md bg-canvas text-xl text-primary",
                                "+"
                            }
                            h3 { class: "font-semibold text-ink", "Upload documents" }
                            p { class: "mt-2 max-w-md text-sm text-body",
                                "Choose markdown, text, CSV, or JSON files. Tayori stores metadata, chunks text, and indexes searchable context."
                            }
                        }
                    }

                    div { class: "min-h-0 overflow-hidden rounded-lg border border-hairline",
                        div { class: "grid grid-cols-[1fr_120px_100px_80px] gap-3 border-b border-hairline bg-surface-soft px-4 py-2 text-xs font-semibold uppercase tracking-[0.08em] text-muted",
                            span { "File" }
                            span { "Type" }
                            span { "Status" }
                            span { "Actions" }
                        }
                        div { class: "max-h-full overflow-auto divide-y divide-hairline",
                            match &*data.read() {
                                Some(Ok(d)) => {
                                    rsx! {
                                        if d.documents.is_empty() {
                                            div { class: "p-6 text-center text-sm text-body", "No documents uploaded yet." }
                                        }
                                        for doc in d.documents.clone() {
                                            div { class: "grid grid-cols-[1fr_120px_100px_80px] gap-3 px-4 py-3 hover:bg-surface-soft",
                                                div {
                                                    p { class: "font-semibold text-ink", "{doc.source_name}" }
                                                    p { class: "text-sm text-body line-clamp-1",
                                                        if doc.status == "failed" {
                                                            "{doc.error_message.clone().unwrap_or_else(|| \"Unknown error\".to_string())}"
                                                        } else if doc.status == "ingesting" {
                                                            "Processing..."
                                                        } else {
                                                            "Indexed"
                                                        }
                                                    }
                                                }
                                                span { class: "self-center text-sm text-body", "Doc" }
                                                span {
                                                    class: match doc.status.as_str() {
                                                        "ready" => {
                                                            "self-center rounded-full bg-surface-strong px-3 py-1 text-center text-xs font-semibold text-ink"
                                                        }
                                                        "failed" => {
                                                            "self-center rounded-full bg-semantic-down/10 px-3 py-1 text-center text-xs font-semibold text-semantic-down"
                                                        }
                                                        _ => {
                                                            "self-center rounded-full bg-surface-strong px-3 py-1 text-center text-xs font-semibold text-ink animate-pulse"
                                                        }
                                                    },
                                                    {
                                                        match doc.status.as_str() {
                                                            "ready" => "Ready",
                                                            "failed" => "Failed",
                                                            _ => "Ingesting...",
                                                        }
                                                    }
                                                }
                                                button {
                                                    class: "self-center rounded-md bg-semantic-down/10 px-3 py-1.5 text-xs font-semibold text-semantic-down hover:bg-semantic-down/20",
                                                    onclick: {
                                                        let doc_id = doc.id.clone();
                                                        let state = state.clone();
                                                        move |_| {
                                                            let model = ProjectPageModel::new(state.clone());
                                                            let doc_id = doc_id.clone();
                                                            spawn(async move {
                                                                if let Err(e) = model.remove_docs(doc_id).await {
                                                                    tracing::error!("Failed to delete document: {e:#}");
                                                                }
                                                                data.restart();
                                                            });
                                                        }
                                                    },
                                                    "Delete"
                                                }
                                            }
                                        }
                                    }
                                }
                                Some(Err(msg)) => rsx! {
                                    ErrorView { message: msg.clone(), on_retry: move |_| data.restart() }
                                },
                                None => rsx! {
                                    div { class: "p-6 text-center text-sm text-body", "Loading..." }
                                },
                            }
                        }
                    }
                }
            }

            aside { class: "flex min-h-0 flex-col overflow-hidden rounded-lg border border-hairline bg-canvas shadow-sm",
                div { class: "flex items-center justify-between border-b border-hairline bg-surface-soft px-4 py-3",
                    div {
                        h3 { class: "font-semibold text-ink", "Sessions" }
                        p { class: "text-sm text-body", "Recordings for this project" }
                    }
                    button {
                        class: "rounded-md bg-primary px-3 py-2 text-sm font-semibold text-on-primary hover:bg-primary-active",
                        onclick: on_create_session,
                        "Create"
                    }
                }
                div { class: "min-h-0 flex-1 overflow-auto p-3",
                    match &*data.read() {
                        Some(Ok(d)) => {
                            rsx! {
                                if d.sessions.is_empty() {
                                    div { class: "p-4 text-center text-sm text-body", "No sessions yet." }
                                }
                                for session in d.sessions.clone() {
                                    div { class: "mb-2 grid grid-cols-[1fr_auto] gap-2 rounded-md px-3 py-3 hover:bg-surface-soft",
                                        button {
                                            class: "text-left",
                                            onclick: {
                                                let pid = session.project_id.clone();
                                                let sid = session.id.clone();
                                                move |_| {
                                                    navigator
                                                        .push(Route::LivePage {
                                                            project_id: pid.clone(),
                                                            session_id: sid.clone(),
                                                        });
                                                }
                                            },
                                            p { class: "font-semibold text-ink",
                                                "{session.title.clone().unwrap_or_else(|| \"Untitled Session\".to_string())}"
                                            }
                                            p { class: "mt-1 text-sm text-body", "{session.created_at.to_string()}" }
                                        }
                                        button {
                                            class: "self-center rounded-md bg-semantic-down/10 px-3 py-1.5 text-xs font-semibold text-semantic-down hover:bg-semantic-down/20",
                                            onclick: {
                                                let sid = session.id.clone();
                                                let state = state.clone();
                                                move |_| {
                                                    let model = ProjectPageModel::new(state.clone());
                                                    let sid = sid.clone();
                                                    spawn(async move {
                                                        if let Err(e) = model.remove_session(sid).await {
                                                            tracing::error!("Failed to delete session: {e:#}");
                                                        }
                                                        data.restart();
                                                    });
                                                }
                                            },
                                            "Delete"
                                        }
                                    }
                                }
                            }
                        }
                        Some(Err(msg)) => rsx! {
                            ErrorView { message: msg.clone(), on_retry: move |_| data.restart() }
                        },
                        None => rsx! {},
                    }
                }
            }
        }
    }
}
