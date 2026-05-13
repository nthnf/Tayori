use dioxus::prelude::*;
use tayori_core::service::{
    DocumentView, LiveSessionEvent, LiveSessionHandle, ProjectView, SessionView, SettingsFormView,
    TayoriCore, TranscriptChunkView, WhisperModelView, default_settings_form_view,
};

use crate::components::TopNav;
use crate::pages::{DashboardPage, LiveRecordingPage, ProjectDetailPage, SettingsPage};
use crate::types::{
    DocumentCard, Page, ProjectCard, ProjectDraft, SessionCard, Theme, TranscriptCard,
};

#[component]
pub fn App() -> Element {
    let mut page = use_signal(|| Page::Dashboard);
    let mut theme = use_signal(|| Theme::Light);
    let core = use_resource(|| async { TayoriCore::bootstrap().await });
    let mut selected_project = use_signal(|| None::<String>);
    let mut selected_session = use_signal(|| None::<String>);
    let mut live_session = use_signal(|| None::<LiveSessionHandle>);
    let mut is_listening = use_signal(|| false);
    let draft = use_signal(|| ProjectDraft {
        name: String::new(),
        description: String::new(),
    });
    let mut projects = use_signal(Vec::<ProjectCard>::new);
    let mut documents = use_signal(Vec::<DocumentCard>::new);
    let mut sessions = use_signal(Vec::<SessionCard>::new);
    let mut transcripts = use_signal(Vec::<TranscriptCard>::new);
    let mut detected_question = use_signal(|| None::<String>);
    let mut suggested_answer = use_signal(|| None::<String>);
    let mut show_missing_api_key = use_signal(|| false);
    let mut settings_form = use_signal(default_settings_form_view);
    let mut saved_settings_form = use_signal(default_settings_form_view);
    let mut whisper_model = use_signal(|| None::<WhisperModelView>);
    let mut app_error = use_signal(|| None::<String>);

    use_effect(move || {
        if let Some(Ok(core)) = core.read().as_ref() {
            let core = core.clone();
            spawn(async move {
                match core.list_projects().await {
                    Ok(rows) => {
                        projects.set(rows.into_iter().map(project_card_from_view).collect())
                    }
                    Err(error) => app_error.set(Some(error.to_string())),
                }
            });
        }
    });

    use_effect(move || {
        if let Some(Ok(core)) = core.read().as_ref() {
            let core = core.clone();
            spawn(async move {
                match core.settings_form().await {
                    Ok(form) => {
                        theme.set(theme_from_settings(&form.ui_theme));
                        settings_form.set(form.clone());
                        saved_settings_form.set(form);
                    }
                    Err(error) => app_error.set(Some(error.to_string())),
                }
            });
        }
    });

    use_effect(move || {
        let Some(project_id) = selected_project() else {
            documents.set(Vec::new());
            sessions.set(Vec::new());
            selected_session.set(None);
            return;
        };

        if let Some(Ok(core)) = core.read().as_ref() {
            let core = core.clone();
            spawn(async move {
                match core.list_documents(&project_id).await {
                    Ok(rows) => {
                        documents.set(rows.into_iter().map(document_card_from_view).collect())
                    }
                    Err(error) => app_error.set(Some(error.to_string())),
                }

                match core.list_sessions(&project_id).await {
                    Ok(rows) => {
                        sessions.set(rows.into_iter().map(session_card_from_view).collect())
                    }
                    Err(error) => app_error.set(Some(error.to_string())),
                }
            });
        }
    });

    use_effect(move || {
        let Some(session_id) = selected_session() else {
            transcripts.set(Vec::new());
            detected_question.set(None);
            suggested_answer.set(None);
            return;
        };

        if let Some(Ok(core)) = core.read().as_ref() {
            let core = core.clone();
            spawn(async move {
                match core.list_transcript_chunks(&session_id).await {
                    Ok(rows) => {
                        let rows: Vec<_> =
                            rows.into_iter().map(transcript_card_from_view).collect();
                        detected_question.set(
                            rows.iter()
                                .rev()
                                .find(|chunk| chunk.has_question)
                                .map(|chunk| chunk.text.clone()),
                        );
                        transcripts.set(rows)
                    }
                    Err(error) => app_error.set(Some(error.to_string())),
                }
            });
        }
    });

    let nav_title = match page() {
        Page::Dashboard => "Projects",
        Page::Project => "Project",
        Page::LiveRecording => "Live Recording",
        Page::Settings => "Settings",
    };

    let current_project = projects
        .read()
        .iter()
        .find(|project| Some(project.id.clone()) == selected_project())
        .cloned();
    let theme_class = match theme() {
        Theme::Light => "theme-light",
        Theme::Dark => "theme-dark",
    };

    let bootstrap_error = core
        .read()
        .as_ref()
        .and_then(|result| result.as_ref().err().map(ToString::to_string));

    rsx! {
        document::Stylesheet { href: asset!("/assets/tailwind.css") }

        main { class: "{theme_class} min-h-screen bg-canvas text-ink font-sans transition-colors duration-200",
            div { class: "flex h-screen w-full flex-col overflow-hidden",
                TopNav { page, title: nav_title.to_string() }
                section { class: "min-h-0 flex-1 overflow-hidden bg-surface-soft p-4",
                    if core.read().is_none() {
                        div { class: "grid h-full place-items-center text-body", "Loading Tayori..." }
                    } else if let Some(error) = bootstrap_error {
                        div { class: "rounded-lg border border-hairline bg-canvas p-5 text-semantic-down", "{error}" }
                    } else {
                        if let Some(error) = app_error() {
                            div { class: "mb-3 rounded-md bg-semantic-down/10 px-3 py-2 text-sm text-semantic-down", "{error}" }
                        }
                        match page() {
                            Page::Dashboard => rsx! {
                                DashboardPage {
                                    draft,
                                    projects,
                                    page,
                                    selected_project,
                                    on_create: move |project_draft: ProjectDraft| {
                                        if let Some(Ok(core)) = core.read().as_ref() {
                                            let core = core.clone();
                                            spawn(async move {
                                                match core.create_project(project_draft.name, non_empty(project_draft.description)).await {
                                                    Ok(row) => {
                                                        let card = project_card_from_view(row);
                                                        selected_project.set(Some(card.id.clone()));
                                                        projects.write().insert(0, card);
                                                        page.set(Page::Project);
                                                    }
                                                    Err(error) => app_error.set(Some(error.to_string())),
                                                }
                                            });
                                        }
                                    }
                                }
                            },
                            Page::Project => rsx! {
                                ProjectDetailPage {
                                    project: current_project,
                                    documents,
                                    sessions,
                                    on_upload_document: move |_| {
                                        let Some(project_id) = selected_project() else { return; };
                                        let Some(path) = rfd::FileDialog::new()
                                            .add_filter("Text documents", &["txt", "md", "markdown", "csv", "json"])
                                            .pick_file() else { return; };

                                        if let Some(Ok(core)) = core.read().as_ref() {
                                            let core = core.clone();
                                            spawn(async move {
                                                match core.upload_document_from_settings(project_id, path).await {
                                                    Ok(document) => documents.write().insert(0, document_card_from_view(document)),
                                                    Err(error) => app_error.set(Some(error.to_string())),
                                                }
                                            });
                                        }
                                    },
                                    on_open_session: move |session_id: String| {
                                        selected_session.set(Some(session_id));
                                        page.set(Page::LiveRecording);
                                    },
                                    on_create_session: move |_| {
                                        if saved_settings_form.read().llm_api_key_preview.is_empty() {
                                            show_missing_api_key.set(true);
                                            return;
                                        }

                                        let Some(project_id) = selected_project() else { return; };
                                        if let Some(Ok(core)) = core.read().as_ref() {
                                            let core = core.clone();
                                            spawn(async move {
                                                match core.create_session(project_id, Some("Live session".to_string())).await {
                                                    Ok(row) => {
                                                        let session = session_card_from_view(row);
                                                        selected_session.set(Some(session.id.clone()));
                                                        sessions.write().insert(0, session);
                                                        page.set(Page::LiveRecording);
                                                    }
                                                    Err(error) => app_error.set(Some(error.to_string())),
                                                }
                                            });
                                        }
                                    }
                                }
                            },
                            Page::LiveRecording => rsx! {
                                LiveRecordingPage {
                                    session: sessions.read().iter().find(|session| Some(session.id.clone()) == selected_session()).cloned(),
                                    transcripts,
                                    detected_question: detected_question(),
                                    suggested_answer: suggested_answer(),
                                    is_listening: is_listening(),
                                    on_start: move |_| {
                                        if is_listening() {
                                            return;
                                        }

                                        if saved_settings_form.read().llm_api_key_preview.is_empty() {
                                            show_missing_api_key.set(true);
                                            return;
                                        }

                                        let Some(project_id) = selected_project() else { return; };
                                        let Some(session_id) = selected_session() else { return; };
                                        let Some(core) = core.read().as_ref().and_then(|result| result.as_ref().ok().cloned()) else { return; };

                                        let handle = match core.start_live_session(project_id, session_id) {
                                            Ok(handle) => handle,
                                            Err(error) => {
                                                app_error.set(Some(error.to_string()));
                                                return;
                                            }
                                        };
                                        let events = handle.events();
                                        live_session.set(Some(handle));
                                        is_listening.set(true);

                                        spawn(async move {
                                            loop {
                                                match events.try_recv() {
                                                    Ok(event) => match event {
                                                        LiveSessionEvent::Transcript(chunk) => transcripts.write().push(transcript_card_from_view(chunk)),
                                                        LiveSessionEvent::QuestionDetected { question, .. } => detected_question.set(Some(question)),
                                                        LiveSessionEvent::AnswerStarted => suggested_answer.set(Some(String::new())),
                                                        LiveSessionEvent::AnswerDelta(delta) => {
                                                            let mut answer = suggested_answer().unwrap_or_default();
                                                            answer.push_str(&delta);
                                                            suggested_answer.set(Some(answer));
                                                        }
                                                        LiveSessionEvent::AnswerFinished(answer) => suggested_answer.set(Some(answer)),
                                                        LiveSessionEvent::Error(error) => app_error.set(Some(error)),
                                                    }
                                                    Err(crossbeam_channel::TryRecvError::Empty) => {
                                                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                                                    }
                                                    Err(crossbeam_channel::TryRecvError::Disconnected) => break,
                                                }
                                            }
                                        });
                                    },
                                    on_pause: move |_| {
                                        if let Some(handle) = live_session.write().take() {
                                            if let Some(Ok(core)) = core.read().as_ref() {
                                                core.pause_live_session(handle);
                                            }
                                        }
                                        is_listening.set(false);
                                    },
                                    on_end: move |_| {
                                        is_listening.set(false);

                                        let Some(session_id) = selected_session() else { return; };
                                        if let Some(Ok(core)) = core.read().as_ref() {
                                            let core = core.clone();
                                            let handle = live_session.write().take();
                                            spawn(async move {
                                                    match core.end_live_session(handle, &session_id).await {
                                                    Ok(()) => {
                                                        sessions.with_mut(|sessions| {
                                                            if let Some(session) = sessions.iter_mut().find(|session| session.id == session_id) {
                                                                session.status = "completed".to_string();
                                                                session.meta = "Completed".to_string();
                                                            }
                                                        });
                                                        page.set(Page::Project);
                                                    }
                                                    Err(error) => app_error.set(Some(error.to_string())),
                                                }
                                            });
                                        }
                                    }
                                }
                            },
                            Page::Settings => rsx! {
                                SettingsPage {
                                    theme,
                                    settings: settings_form,
                                    saved_settings: saved_settings_form,
                                    whisper_model,
                                    on_check_whisper_model: move |model_name: String| {
                                        if let Some(Ok(core)) = core.read().as_ref() {
                                            match core.whisper_model_status(&model_name) {
                                                Ok(status) => whisper_model.set(Some(status)),
                                                Err(error) => app_error.set(Some(error.to_string())),
                                            }
                                        }
                                    },
                                    on_install_whisper_model: move |model_name: String| {
                                        if let Some(Ok(core)) = core.read().as_ref() {
                                            let core = core.clone();
                                            spawn(async move {
                                                match tokio::task::spawn_blocking(move || core.install_whisper_model_by_name(&model_name)).await {
                                                    Ok(Ok(status)) => whisper_model.set(Some(status)),
                                                    Ok(Err(error)) => app_error.set(Some(error.to_string())),
                                                    Err(error) => app_error.set(Some(error.to_string())),
                                                }
                                            });
                                        }
                                    },
                                    on_remove_whisper_model: move |model_name: String| {
                                        if let Some(Ok(core)) = core.read().as_ref() {
                                            match core.remove_whisper_model_by_name(&model_name) {
                                                Ok(status) => whisper_model.set(Some(status)),
                                                Err(error) => app_error.set(Some(error.to_string())),
                                            }
                                        }
                                    },
                                    on_save: move |form: SettingsFormView| {
                                        if let Some(Ok(core)) = core.read().as_ref() {
                                            let core = core.clone();
                                            spawn(async move {
                                                match core.update_settings_form(form).await {
                                                    Ok(form) => {
                                                        theme.set(theme_from_settings(&form.ui_theme));
                                                        settings_form.set(form.clone());
                                                        saved_settings_form.set(form);
                                                    }
                                                    Err(error) => app_error.set(Some(error.to_string())),
                                                }
                                            });
                                        }
                                    }
                                }
                            },
                        }
                    }

                    if show_missing_api_key() {
                        div { class: "absolute inset-0 z-20 grid place-items-center bg-black/30 p-4",
                            div { class: "w-full max-w-md rounded-lg border border-hairline bg-canvas p-5 shadow-2xl",
                                h3 { class: "text-lg font-semibold text-ink", "API key required" }
                                p { class: "mt-2 text-sm leading-relaxed text-body", "Add an LLM API key before starting a live session so Tayori can generate suggested answers when questions are detected." }
                                div { class: "mt-5 flex justify-end gap-2",
                                    button { class: "rounded-md px-4 py-2 text-sm font-semibold text-body hover:bg-surface-soft", onclick: move |_| show_missing_api_key.set(false), "Cancel" }
                                    button { class: "rounded-md bg-primary px-4 py-2 text-sm font-semibold text-on-primary hover:bg-primary-active", onclick: move |_| { show_missing_api_key.set(false); page.set(Page::Settings); }, "Add API key" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn project_card_from_view(project: ProjectView) -> ProjectCard {
    ProjectCard {
        id: project.id,
        name: project.name,
        description: project.description,
    }
}

fn session_card_from_view(session: SessionView) -> SessionCard {
    SessionCard {
        id: session.id,
        title: session.title,
        status: session.status,
        meta: session.meta,
    }
}

fn document_card_from_view(document: DocumentView) -> DocumentCard {
    DocumentCard {
        id: document.id,
        name: document.name,
        meta: document.meta,
        kind: document.kind,
        status: document.status,
    }
}

fn transcript_card_from_view(chunk: TranscriptChunkView) -> TranscriptCard {
    TranscriptCard {
        id: chunk.id,
        text: chunk.text,
        time: chunk.time,
        chunk_index: chunk.chunk_index,
        start_ms: chunk.start_ms,
        end_ms: chunk.end_ms,
        duration_ms: chunk.duration_ms,
        has_question: chunk.has_question,
        confidence: chunk.confidence,
    }
}

fn non_empty(value: String) -> Option<String> {
    let value = value.trim().to_string();
    if value.is_empty() { None } else { Some(value) }
}

fn theme_from_settings(value: &str) -> Theme {
    if value == "dark" {
        Theme::Dark
    } else {
        Theme::Light
    }
}
