use dioxus::prelude::*;

use crate::components::TopNav;
use crate::pages::{DashboardPage, LiveRecordingPage, ProjectDetailPage, SettingsPage};
use crate::types::{Page, ProjectCard, ProjectDraft, Theme};

#[component]
pub fn App() -> Element {
    let page = use_signal(|| Page::Dashboard);
    let theme = use_signal(|| Theme::Light);
    let selected_project = use_signal(|| None::<usize>);
    let draft = use_signal(|| ProjectDraft {
        name: String::new(),
        description: String::new(),
    });
    let projects = use_signal(|| {
        vec![
            ProjectCard {
                id: 1,
                name: "Q3 Institutional Audit".to_string(),
                description: "Interview notes, operating docs, and recurring review sessions."
                    .to_string(),
            },
            ProjectCard {
                id: 2,
                name: "Acme Onboarding".to_string(),
                description: "Customer calls, kickoff material, and implementation decisions."
                    .to_string(),
            },
        ]
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
        .find(|project| Some(project.id) == selected_project())
        .cloned();
    let theme_class = match theme() {
        Theme::Light => "theme-light",
        Theme::Dark => "theme-dark",
    };

    rsx! {
        document::Stylesheet { href: asset!("/assets/tailwind.css") }

        main { class: "{theme_class} min-h-screen bg-canvas text-ink font-sans transition-colors duration-200",
            div { class: "flex h-screen w-full flex-col overflow-hidden",
                TopNav { page, title: nav_title.to_string() }
                section { class: "min-h-0 flex-1 overflow-hidden bg-surface-soft p-4",
                        match page() {
                            Page::Dashboard => rsx! { DashboardPage { draft, projects, page, selected_project } },
                            Page::Project => rsx! { ProjectDetailPage { project: current_project, page } },
                            Page::LiveRecording => rsx! { LiveRecordingPage {} },
                            Page::Settings => rsx! { SettingsPage { theme } },
                        }
                }
            }
        }
    }
}
