use dioxus::prelude::*;

use crate::types::{Page, ProjectCard, ProjectDraft};

#[component]
pub fn DashboardPage(
    mut draft: Signal<ProjectDraft>,
    mut projects: Signal<Vec<ProjectCard>>,
    mut page: Signal<Page>,
    mut selected_project: Signal<Option<usize>>,
) -> Element {
    let mut show_create = use_signal(|| false);
    let can_create = !draft.read().name.trim().is_empty();
    let project_cards = projects.read().clone();
    let recent_projects = project_cards.clone();

    let mut create_project = move |_| {
        let current = draft.read().clone();
        let name = current.name.trim().to_string();
        if name.is_empty() {
            return;
        }

        let id = projects.read().len() + 1;
        projects.write().push(ProjectCard {
            id,
            name,
            description: current.description.trim().to_string(),
        });
        draft.set(ProjectDraft {
            name: String::new(),
            description: String::new(),
        });
        show_create.set(false);
    };

    rsx! {
        div { class: "relative flex h-full flex-col overflow-hidden rounded-lg border border-hairline bg-canvas shadow-sm",
            div { class: "flex items-center justify-between border-b border-hairline bg-surface-soft px-4 py-3",
                div {
                    h2 { class: "text-lg font-semibold text-ink", "Project Manager" }
                    p { class: "text-sm text-body", "Select an existing project or create a new one." }
                }
                button { class: "rounded-md bg-primary px-4 py-2 text-sm font-semibold text-on-primary hover:bg-primary-active", onclick: move |_| show_create.set(true), "New Project" }
            }

            div { class: "grid flex-1 overflow-hidden md:grid-cols-[260px_minmax(0,1fr)]",
                aside { class: "hidden border-r border-hairline bg-surface-soft p-4 md:block",
                    h3 { class: "mb-3 text-xs font-semibold uppercase tracking-[0.08em] text-muted", "Recent Projects" }
                    div { class: "grid gap-1",
                        for project in recent_projects.into_iter().take(6) {
                            button {
                                class: "truncate rounded-md px-3 py-2 text-left text-sm text-body hover:bg-surface-strong hover:text-ink",
                                onclick: move |_| {
                                    selected_project.set(Some(project.id));
                                    page.set(Page::Project);
                                },
                                "{project.name}"
                            }
                        }
                    }
                }

                section { class: "min-h-0 overflow-auto p-4",
                    div { class: "mb-3 grid grid-cols-[1fr_180px_120px] gap-3 border-b border-hairline px-3 pb-2 text-xs font-semibold uppercase tracking-[0.08em] text-muted",
                        span { "Name" }
                        span { "Last edited" }
                        span { "Status" }
                    }
                    div { class: "grid gap-1",
                        if project_cards.is_empty() {
                            div { class: "rounded-md border border-dashed border-hairline p-8 text-center text-body", "No projects yet. Create one to begin." }
                        }
                        for project in project_cards {
                            button {
                                class: "grid grid-cols-[1fr_180px_120px] gap-3 rounded-md px-3 py-3 text-left hover:bg-surface-soft",
                                onclick: move |_| {
                                    selected_project.set(Some(project.id));
                                    page.set(Page::Project);
                                },
                                div {
                                    p { class: "font-semibold text-ink", "{project.name}" }
                                    p { class: "truncate text-sm text-body", if project.description.is_empty() { "No description" } else { "{project.description}" } }
                                }
                                span { class: "self-center text-sm text-body", "Today" }
                                span { class: "self-center rounded-full bg-surface-strong px-3 py-1 text-center text-xs font-semibold text-ink", "Ready" }
                            }
                        }
                    }
                }
            }

            if show_create() {
                div { class: "absolute inset-0 z-10 grid place-items-center bg-black/25 p-4",
                    div { class: "w-full max-w-md rounded-lg border border-hairline bg-canvas p-5 shadow-2xl",
                        div { class: "mb-5 flex items-center justify-between",
                            h3 { class: "text-lg font-semibold text-ink", "Create Project" }
                            button { class: "rounded-md px-2 py-1 text-body hover:bg-surface-soft", onclick: move |_| show_create.set(false), "Esc" }
                        }
                        div { class: "grid gap-4",
                            label { class: "grid gap-2",
                                span { class: "text-sm font-semibold text-ink", "Name" }
                                input { class: "rounded-md border border-hairline bg-canvas px-3 py-2 text-ink outline-none focus:border-primary", value: "{draft.read().name}", oninput: move |event| draft.write().name = event.value() }
                            }
                            label { class: "grid gap-2",
                                span { class: "text-sm font-semibold text-ink", "Description" }
                                textarea { class: "min-h-24 resize-none rounded-md border border-hairline bg-canvas px-3 py-2 text-ink outline-none focus:border-primary", value: "{draft.read().description}", oninput: move |event| draft.write().description = event.value() }
                            }
                            div { class: "flex justify-end gap-2 pt-2",
                                button { class: "rounded-md px-4 py-2 text-sm font-semibold text-body hover:bg-surface-soft", onclick: move |_| show_create.set(false), "Cancel" }
                                button { class: if can_create { "rounded-md bg-primary px-4 py-2 text-sm font-semibold text-on-primary hover:bg-primary-active" } else { "cursor-not-allowed rounded-md bg-primary-disabled px-4 py-2 text-sm font-semibold text-on-primary" }, disabled: !can_create, onclick: move |event| create_project(event), "Create" }
                            }
                        }
                    }
                }
            }
        }
    }
}
