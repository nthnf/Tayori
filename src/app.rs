use dioxus::prelude::*;
use tayori_core::AudioRuntime;
use tayori_core::service::{
    DocumentView, ProjectView, SessionView, SettingsUpdate, SettingsView, TayoriCore,
    TranscriptChunkView,
};

use crate::components::TopNav;
use crate::pages::{DashboardPage, LiveRecordingPage, ProjectDetailPage, SettingsPage};
use crate::types::{
    DocumentCard, Page, ProjectCard, ProjectDraft, SessionCard, SettingsForm, Theme, TranscriptCard,
};

#[component]
pub fn App() -> Element {
    let mut page = use_signal(|| Page::Dashboard);
    let mut theme = use_signal(|| Theme::Light);
    let core = use_resource(|| async { TayoriCore::bootstrap().await });
    let mut selected_project = use_signal(|| None::<String>);
    let mut selected_session = use_signal(|| None::<String>);
    let mut audio_runtime = use_signal(|| None::<AudioRuntime>);
    let mut is_listening = use_signal(|| false);
    let draft = use_signal(|| ProjectDraft {
        name: String::new(),
        description: String::new(),
    });
    let mut projects = use_signal(Vec::<ProjectCard>::new);
    let mut documents = use_signal(Vec::<DocumentCard>::new);
    let mut sessions = use_signal(Vec::<SessionCard>::new);
    let mut transcripts = use_signal(Vec::<TranscriptCard>::new);
    let mut settings_form = use_signal(default_settings_form);
    let mut saved_settings_form = use_signal(default_settings_form);
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
                match core.settings().await {
                    Ok(settings) => {
                        let form = settings_form_from_view(settings);
                        theme.set(form.ui_theme);
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
            return;
        };

        if let Some(Ok(core)) = core.read().as_ref() {
            let core = core.clone();
            spawn(async move {
                match core.list_transcript_chunks(&session_id).await {
                    Ok(rows) => {
                        transcripts.set(rows.into_iter().map(transcript_card_from_view).collect())
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
                                    is_listening: is_listening(),
                                    on_start: move |_| {
                                        if is_listening() {
                                            return;
                                        }

                                        let Some(project_id) = selected_project() else { return; };
                                        let Some(session_id) = selected_session() else { return; };
                                        let Some(core) = core.read().as_ref().and_then(|result| result.as_ref().ok().cloned()) else { return; };

                                        let mut runtime = match AudioRuntime::start_default() {
                                            Ok(runtime) => runtime,
                                            Err(error) => {
                                                app_error.set(Some(error.to_string()));
                                                return;
                                            }
                                        };
                                        let transcriptions = runtime.transcriptions();
                                        if let Err(error) = runtime.start() {
                                            app_error.set(Some(error.to_string()));
                                            return;
                                        }

                                        audio_runtime.set(Some(runtime));
                                        is_listening.set(true);

                                        spawn(async move {
                                            loop {
                                                match transcriptions.try_recv() {
                                                    Ok(chunk) => {
                                                        match core.persist_transcription_chunk(&project_id, &session_id, chunk).await {
                                                            Ok(chunk) => transcripts.write().push(transcript_card_from_view(chunk)),
                                                            Err(error) => app_error.set(Some(error.to_string())),
                                                        }
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
                                        if let Some(mut runtime) = audio_runtime.write().take() {
                                            runtime.stop();
                                        }
                                        is_listening.set(false);
                                    },
                                    on_end: move |_| {
                                        if let Some(mut runtime) = audio_runtime.write().take() {
                                            runtime.stop();
                                        }
                                        is_listening.set(false);

                                        let Some(session_id) = selected_session() else { return; };
                                        if let Some(Ok(core)) = core.read().as_ref() {
                                            let core = core.clone();
                                            spawn(async move {
                                                match core.end_session(&session_id).await {
                                                    Ok(()) => page.set(Page::Project),
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
                                    on_save: move |form: SettingsForm| {
                                        if let Some(Ok(core)) = core.read().as_ref() {
                                            let core = core.clone();
                                            spawn(async move {
                                                match core.update_settings(settings_update_from_form(form)).await {
                                                    Ok(settings) => {
                                                        let form = settings_form_from_view(settings);
                                                        theme.set(form.ui_theme);
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
    }
}

fn non_empty(value: String) -> Option<String> {
    let value = value.trim().to_string();
    if value.is_empty() { None } else { Some(value) }
}

fn default_settings_form() -> SettingsForm {
    SettingsForm {
        llm_provider: "OpenAI-compatible".to_string(),
        llm_model: "gpt-4.1-mini".to_string(),
        embedding_provider: "FastEmbed".to_string(),
        embedding_model: "jinaai/jina-embeddings-v2-base-code".to_string(),
        sparse_model: "prithivida/splade_pp_en_v1".to_string(),
        reranker_model: "jinaai/jina-reranker-v1-turbo-en".to_string(),
        whisper_model: "small-q8_0".to_string(),
        summary_minutes: "5".to_string(),
        ui_theme: Theme::Light,
    }
}

fn settings_form_from_view(settings: SettingsView) -> SettingsForm {
    SettingsForm {
        llm_provider: settings.llm_provider,
        llm_model: settings.llm_model,
        embedding_provider: settings.embedding_provider,
        embedding_model: settings.embedding_model,
        sparse_model: settings.sparse_model,
        reranker_model: settings.reranker_model,
        whisper_model: settings.whisper_model,
        summary_minutes: settings.summary_minutes.to_string(),
        ui_theme: theme_from_settings(&settings.ui_theme),
    }
}

fn settings_update_from_form(form: SettingsForm) -> SettingsUpdate {
    SettingsUpdate {
        llm_model: form.llm_model,
        embedding_model: form.embedding_model,
        sparse_model: form.sparse_model,
        reranker_model: form.reranker_model,
        whisper_model: form.whisper_model,
        summary_minutes: form.summary_minutes.parse::<i64>().unwrap_or(5),
        ui_theme: match form.ui_theme {
            Theme::Light => "light".to_string(),
            Theme::Dark => "dark".to_string(),
        },
    }
}

fn theme_from_settings(value: &str) -> Theme {
    if value == "dark" {
        Theme::Dark
    } else {
        Theme::Light
    }
}
