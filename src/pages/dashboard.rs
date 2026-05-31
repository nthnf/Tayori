use crate::Route;
use crate::backend::pages::dashboard::DashboardPageModel;
use crate::components::error::ErrorView;
use crate::components::project_card::{EmptyProjectCard, ProjectCardData, ProjectCardView};
use crate::state::AppState;
use dioxus::prelude::*;

#[component]
pub fn DashboardPage() -> Element {
    let state = use_context::<AppState>();
    let mut show_create = use_signal(|| false);
    let mut edit_project_id = use_signal(|| None::<String>);
    let mut draft_name = use_signal(String::new);
    let mut draft_desc = use_signal(String::new);
    let mut action_error = use_signal(|| None::<String>);
    let navigator = use_navigator();

    // Load data
    let state_for_load = state.clone();
    let mut data = use_resource(move || {
        let model = DashboardPageModel::new(state_for_load.clone());
        async move { model.load().await.map_err(|e| format!("{e:#}")) }
    });

    let state_for_action = state.clone();
    let save_project = move |_| {
        let name = draft_name.read().trim().to_string();
        if name.is_empty() {
            return;
        }

        let desc = draft_desc.read().trim().to_string();
        let editing_id = edit_project_id.read().clone();

        let model = DashboardPageModel::new(state_for_action.clone());
        spawn(async move {
            if let Some(id) = editing_id {
                if let Err(e) = model.update_project(id, name, desc).await {
                    tracing::error!("Failed to update project: {e:#}");
                    action_error.set(Some(e.to_string()));
                    return;
                }
            } else {
                if let Err(e) = model.create_project(name, desc).await {
                    tracing::error!("Failed to create project: {e:#}");
                    action_error.set(Some(e.to_string()));
                    return;
                }
            }
            action_error.set(None);
            data.restart();
        });

        draft_name.set(String::new());
        draft_desc.set(String::new());
        show_create.set(false);
        edit_project_id.set(None);
    };

    let can_create = !draft_name.read().trim().is_empty();

    rsx! {
        div { class: "relative flex h-full flex-col overflow-hidden rounded-lg border border-hairline bg-canvas shadow-sm",
            div { class: "flex items-center justify-between border-b border-hairline bg-surface-soft px-4 py-3",
                div {
                    h2 { class: "text-lg font-semibold text-ink", "Project Manager" }
                    p { class: "text-sm text-body", "Select an existing project or create a new one." }
                }
                button {
                    class: "rounded-md bg-primary px-4 py-2 text-sm font-semibold text-on-primary hover:bg-primary-active",
                    onclick: move |_| {
                        draft_name.set(String::new());
                        draft_desc.set(String::new());
                        edit_project_id.set(None);
                        show_create.set(true);
                    },
                    "New Project"
                }
            }

            div { class: "grid flex-1 overflow-hidden md:grid-cols-[260px_minmax(0,1fr)]",
                aside { class: "hidden border-r border-hairline bg-surface-soft p-4 md:block overflow-y-auto",
                    h3 { class: "mb-3 text-xs font-semibold uppercase tracking-[0.08em] text-muted",
                        "Recent Projects"
                    }
                    div { class: "grid gap-1",
                        match &*data.read() {
                            Some(Ok(d)) => {
                                rsx! {
                                    for project in d.projects.iter().take(6) {
                                        button {
                                            class: "truncate rounded-md px-3 py-2 text-left text-sm text-body hover:bg-surface-strong hover:text-ink w-full",
                                            onclick: {
                                                let id = project.id.clone();
                                                move |_| {
                                                    navigator.push(Route::ProjectPage {
                                                        id: id.clone(),
                                                    });
                                                }
                                            },
                                            "{project.name}"
                                        }
                                    }
                                }
                            }
                            Some(Err(_)) => rsx! {
                                div { class: "text-sm text-semantic-down", "Failed to load" }
                            },
                            None => rsx! {
                                div { class: "text-sm text-body", "Loading..." }
                            },
                        }
                    }
                }

                section { class: "min-h-0 overflow-auto p-4 flex flex-col gap-4",
                    if let Some(err) = action_error() {
                        ErrorView {
                            message: err,
                            on_retry: move |_| action_error.set(None),
                        }
                    }
                    div { class: "border-b border-hairline px-3 pb-2 text-xs font-semibold uppercase tracking-[0.08em] text-muted",
                        span { "Name" }
                    }
                    div { class: "grid gap-1",
                        match &*data.read() {
                            Some(Ok(d)) => {
                                rsx! {
                                    if d.projects.is_empty() {
                                        EmptyProjectCard {}
                                    }
                                    for project in d.projects.clone() {
                                        ProjectCardView {
                                            project: ProjectCardData {
                                                id: project.id.clone(),
                                                name: project.name.clone(),
                                                description: project.description.clone().unwrap_or_default(),
                                            },
                                            onclick: {
                                                let id = project.id.clone();
                                                move |_| {
                                                    navigator.push(Route::ProjectPage {
                                                        id: id.clone(),
                                                    });
                                                }
                                            },
                                            onedit: {
                                                let id = project.id.clone();
                                                let p_name = project.name.clone();
                                                let p_desc = project.description.clone().unwrap_or_default();
                                                move |_| {
                                                    edit_project_id.set(Some(id.clone()));
                                                    draft_name.set(p_name.clone());
                                                    draft_desc.set(p_desc.clone());
                                                    show_create.set(true);
                                                }
                                            },
                                            ondelete: {
                                                let id = project.id.clone();
                                                let state_for_delete = state.clone();
                                                move |_| {
                                                    let model = DashboardPageModel::new(state_for_delete.clone());
                                                    let delete_id = id.clone();
                                                    spawn(async move {
                                                        if let Err(e) = model.delete_project(delete_id).await {
                                                            tracing::error!("Failed to delete project: {e:#}");
                                                            action_error.set(Some(e.to_string()));
                                                            return;
                                                        }
                                                        action_error.set(None);
                                                        data.restart();
                                                    });
                                                }
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

            if show_create() {
                div { class: "absolute inset-0 z-10 grid place-items-center bg-black/25 p-4",
                    div { class: "w-full max-w-md rounded-lg border border-hairline bg-canvas p-5 shadow-2xl",
                        div { class: "mb-5 flex items-center justify-between",
                            h3 { class: "text-lg font-semibold text-ink", if edit_project_id().is_some() { "Update Project" } else { "Create Project" } }
                            button {
                                class: "rounded-md px-2 py-1 text-body hover:bg-surface-soft",
                                onclick: move |_| {
                                    show_create.set(false);
                                    edit_project_id.set(None);
                                },
                                "Esc"
                            }
                        }
                        div { class: "grid gap-4",
                            label { class: "grid gap-2",
                                span { class: "text-sm font-semibold text-ink", "Name" }
                                input {
                                    class: "rounded-md border border-hairline bg-canvas px-3 py-2 text-ink outline-none focus:border-primary",
                                    value: "{draft_name}",
                                    oninput: move |event| draft_name.set(event.value().clone()),
                                    autofocus: true,
                                }
                            }
                            label { class: "grid gap-2",
                                span { class: "text-sm font-semibold text-ink", "Description" }
                                textarea {
                                    class: "min-h-24 resize-none rounded-md border border-hairline bg-canvas px-3 py-2 text-ink outline-none focus:border-primary",
                                    value: "{draft_desc}",
                                    oninput: move |event| draft_desc.set(event.value().clone()),
                                }
                            }
                            div { class: "flex justify-end gap-2 pt-2",
                                button {
                                    class: "rounded-md px-4 py-2 text-sm font-semibold text-body hover:bg-surface-soft",
                                    onclick: move |_| {
                                        show_create.set(false);
                                        edit_project_id.set(None);
                                    },
                                    "Cancel"
                                }
                                button {
                                    class: if can_create { "rounded-md bg-primary px-4 py-2 text-sm font-semibold text-on-primary hover:bg-primary-active" } else { "cursor-not-allowed rounded-md bg-primary-disabled px-4 py-2 text-sm font-semibold text-on-primary" },
                                    disabled: !can_create,
                                    onclick: save_project,
                                    if edit_project_id().is_some() { "Save" } else { "Create" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
