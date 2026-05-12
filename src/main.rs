use dioxus::prelude::*;

fn main() {
    dioxus::launch(App);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Page {
    Projects,
    Settings,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Theme {
    Light,
    Dark,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ProjectDraft {
    name: String,
    description: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ProjectCard {
    name: String,
    description: String,
}

#[component]
fn App() -> Element {
    let mut page = use_signal(|| Page::Projects);
    let mut theme = use_signal(|| Theme::Light);
    let draft = use_signal(|| ProjectDraft {
        name: String::new(),
        description: String::new(),
    });
    let projects = use_signal(Vec::<ProjectCard>::new);

    let theme_class = match theme() {
        Theme::Light => "theme-light",
        Theme::Dark => "theme-dark",
    };

    let page_title = match page() {
        Page::Projects => "Projects",
        Page::Settings => "Settings",
    };

    rsx! {
        document::Stylesheet { href: asset!("/assets/tailwind.css") }

        main { class: "{theme_class} min-h-screen bg-canvas text-ink font-sans transition-colors",
            div { class: "mx-auto flex min-h-screen w-full max-w-shell flex-col px-base py-base lg:px-xl",
                TopNav { page, theme }

                section { class: "grid flex-1 gap-xl py-section lg:grid-cols-[280px_minmax(0,1fr)]",
                    aside { class: "product-ui-card-light h-fit p-xl",
                        div { class: "mb-xl",
                            p { class: "badge-pill mb-sm", "Workspace" }
                            h1 { class: "display-sm", "Tayori" }
                            p { class: "mt-sm text-body", "Meeting memory, document context, and calm retrieval in one local workspace." }
                        }

                        nav { class: "grid gap-xs",
                            NavButton {
                                active: page() == Page::Projects,
                                label: "Projects".to_string(),
                                onclick: move |_| page.set(Page::Projects),
                            }
                            NavButton {
                                active: page() == Page::Settings,
                                label: "Settings".to_string(),
                                onclick: move |_| page.set(Page::Settings),
                            }
                        }
                    }

                    div { class: "min-w-0",
                        div { class: "mb-xl flex flex-col gap-sm md:flex-row md:items-end md:justify-between",
                            div {
                                p { class: "badge-pill mb-sm", "Local-first" }
                                h2 { class: "display-lg", "{page_title}" }
                            }
                            div { class: "flex gap-sm",
                                ThemeButton { selected: theme() == Theme::Light, label: "Light".to_string(), onclick: move |_| theme.set(Theme::Light) }
                                ThemeButton { selected: theme() == Theme::Dark, label: "Dark".to_string(), onclick: move |_| theme.set(Theme::Dark) }
                            }
                        }

                        if page() == Page::Projects {
                            ProjectsPage { draft, projects }
                        } else {
                            SettingsPage { theme }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn TopNav(mut page: Signal<Page>, theme: Signal<Theme>) -> Element {
    let on_dark = theme() == Theme::Dark;
    let nav_class = if on_dark {
        "top-nav top-nav-on-dark"
    } else {
        "top-nav top-nav-light"
    };

    rsx! {
        header { class: "{nav_class}",
            button { class: "brand-wordmark", onclick: move |_| page.set(Page::Projects), "Tayori" }
            div { class: "hidden items-center gap-lg md:flex",
                span { class: "nav-link", "Capture" }
                span { class: "nav-link", "Documents" }
                span { class: "nav-link", "Search" }
            }
            div { class: "flex items-center gap-sm",
                button { class: "button-secondary-light", onclick: move |_| page.set(Page::Settings), "Settings" }
                button { class: "button-primary", onclick: move |_| page.set(Page::Projects), "New project" }
            }
        }
    }
}

#[component]
fn ProjectsPage(
    mut draft: Signal<ProjectDraft>,
    mut projects: Signal<Vec<ProjectCard>>,
) -> Element {
    let can_create = !draft.read().name.trim().is_empty();

    let create_project = move |_| {
        let current = draft.read().clone();
        let name = current.name.trim().to_string();
        if name.is_empty() {
            return;
        }

        projects.write().push(ProjectCard {
            name,
            description: current.description.trim().to_string(),
        });
        draft.set(ProjectDraft {
            name: String::new(),
            description: String::new(),
        });
    };

    rsx! {
        div { class: "grid gap-xl xl:grid-cols-[minmax(0,1fr)_360px]",
            section { class: "hero-band-dark overflow-hidden",
                div { class: "relative z-10 max-w-2xl",
                    p { class: "badge-pill-dark mb-base", "Project memory" }
                    h3 { class: "display-xl text-on-dark", "Create a calm workspace for every meeting thread." }
                    p { class: "mt-lg max-w-xl text-on-dark-soft", "Each project will group documents, transcripts, summaries, and retrieval context without mixing client or research domains." }
                }

                div { class: "mock-card-stack",
                    div { class: "product-ui-card-dark rotate-card-a",
                        p { class: "caption-strong text-on-dark-soft", "Active context" }
                        h4 { class: "mt-sm title-md text-on-dark", "Q2 hiring sync" }
                        div { class: "mt-lg grid gap-sm",
                            div { class: "mock-row w-11/12" }
                            div { class: "mock-row w-9/12" }
                            div { class: "mock-row w-10/12" }
                        }
                    }
                    div { class: "product-ui-card-dark rotate-card-b",
                        p { class: "caption-strong text-on-dark-soft", "RAG ready" }
                        h4 { class: "mt-sm title-md text-on-dark", "12 summaries indexed" }
                    }
                }
            }

            section { class: "product-ui-card-light p-xl",
                p { class: "badge-pill mb-base", "Create" }
                h3 { class: "title-lg", "New project" }
                p { class: "mt-xs text-body", "Start with a clear name. Documents and meetings can be attached after creation." }

                div { class: "mt-xl grid gap-base",
                    label { class: "grid gap-xs",
                        span { class: "title-sm", "Project name" }
                        input {
                            class: "text-input",
                            value: "{draft.read().name}",
                            placeholder: "Acme onboarding",
                            oninput: move |event| draft.write().name = event.value(),
                        }
                    }

                    label { class: "grid gap-xs",
                        span { class: "title-sm", "Description" }
                        textarea {
                            class: "text-input min-h-[120px] resize-none",
                            value: "{draft.read().description}",
                            placeholder: "Internal meetings, customer notes, and launch docs.",
                            oninput: move |event| draft.write().description = event.value(),
                        }
                    }

                    button {
                        class: if can_create { "button-primary w-full" } else { "button-primary-disabled w-full" },
                        disabled: !can_create,
                        onclick: create_project,
                        "Create project"
                    }
                }
            }
        }

        section { class: "mt-xl grid gap-base md:grid-cols-2 xl:grid-cols-3",
            if projects.read().is_empty() {
                EmptyProjectCard {}
            }

            for project in projects.read().iter() {
                ProjectCardView { project: project.clone() }
            }
        }
    }
}

#[component]
fn SettingsPage(theme: Signal<Theme>) -> Element {
    let active_theme = match theme() {
        Theme::Light => "Light",
        Theme::Dark => "Dark",
    };

    rsx! {
        div { class: "grid gap-xl lg:grid-cols-2",
            section { class: "product-ui-card-light p-xl",
                p { class: "badge-pill mb-base", "Appearance" }
                h3 { class: "title-lg", "Theme" }
                p { class: "mt-xs text-body", "Default is light. Dark mode keeps the Coinbase-style editorial canvas for focused work." }

                div { class: "mt-xl grid gap-sm",
                    SettingRow { label: "Active theme".to_string(), value: active_theme.to_string() }
                    SettingRow { label: "Default theme".to_string(), value: "Light".to_string() }
                    SettingRow { label: "Accent token".to_string(), value: "primary / #0052ff".to_string() }
                }
            }

            section { class: "product-ui-card-light p-xl",
                p { class: "badge-pill mb-base", "Retrieval" }
                h3 { class: "title-lg", "Index defaults" }
                p { class: "mt-xs text-body", "These match the storage defaults and shape how transcript summaries and document chunks become searchable." }

                div { class: "mt-xl grid gap-sm",
                    SettingRow { label: "Summary window".to_string(), value: "5 minutes".to_string() }
                    SettingRow { label: "Embedding dimension".to_string(), value: "768".to_string() }
                    SettingRow { label: "Document storage".to_string(), value: "Path reference only".to_string() }
                }
            }
        }
    }
}

#[component]
fn NavButton(active: bool, label: String, onclick: EventHandler<MouseEvent>) -> Element {
    rsx! {
        button {
            class: if active { "nav-button nav-button-active" } else { "nav-button" },
            onclick: move |event| onclick.call(event),
            "{label}"
        }
    }
}

#[component]
fn ThemeButton(selected: bool, label: String, onclick: EventHandler<MouseEvent>) -> Element {
    rsx! {
        button {
            class: if selected { "theme-button theme-button-active" } else { "theme-button" },
            onclick: move |event| onclick.call(event),
            "{label}"
        }
    }
}

#[component]
fn SettingRow(label: String, value: String) -> Element {
    rsx! {
        div { class: "flex items-center justify-between rounded-md bg-surface-soft px-base py-sm",
            span { class: "text-body", "{label}" }
            span { class: "number-display text-ink", "{value}" }
        }
    }
}

#[component]
fn EmptyProjectCard() -> Element {
    rsx! {
        div { class: "feature-card border-dashed",
            div { class: "asset-icon-circular mb-lg", "＋" }
            h3 { class: "title-md", "No projects yet" }
            p { class: "mt-sm text-body", "Create your first workspace to start attaching meetings and documents." }
        }
    }
}

#[component]
fn ProjectCardView(project: ProjectCard) -> Element {
    rsx! {
        article { class: "feature-card",
            div { class: "mb-lg flex items-center justify-between",
                div { class: "asset-icon-circular", "T" }
                span { class: "badge-pill", "Ready" }
            }
            h3 { class: "title-md", "{project.name}" }
            p { class: "mt-sm text-body", if project.description.is_empty() { "No description yet." } else { "{project.description}" } }
            div { class: "mt-xl flex gap-sm",
                button { class: "button-secondary-light", "Open" }
                button { class: "button-tertiary-text", "Add docs" }
            }
        }
    }
}
