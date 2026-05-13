use dioxus::prelude::*;

use crate::types::ProjectCard;

#[component]
pub fn EmptyProjectCard() -> Element {
    rsx! {
        div { class: "rounded-[24px] border border-dashed border-hairline bg-canvas p-8 text-ink",
            div { class: "mb-6 flex h-10 w-10 items-center justify-center rounded-full bg-surface-strong font-bold text-primary", "+" }
            h3 { class: "text-lg font-semibold", "No projects yet" }
            p { class: "mt-3 text-body", "Create your first workspace to start attaching meetings and documents." }
        }
    }
}

#[component]
pub fn ProjectCardView(project: ProjectCard, onclick: EventHandler<MouseEvent>) -> Element {
    rsx! {
        article {
            class: "group cursor-pointer rounded-[24px] border border-hairline bg-canvas p-6 text-ink shadow-sm transition hover:-translate-y-0.5 hover:border-primary hover:shadow-md",
            onclick: move |event| onclick.call(event),
            div { class: "mb-6 flex items-center justify-between",
                div { class: "flex h-11 w-11 items-center justify-center rounded-2xl bg-surface-strong font-bold text-primary", "T" }
                span { class: "inline-flex rounded-full bg-surface-strong px-3 py-1.5 text-xs font-semibold uppercase tracking-[0.04em] text-ink", "Ready" }
            }
            h3 { class: "text-lg font-semibold", "{project.name}" }
            p { class: "mt-3 text-body", if project.description.is_empty() { "No description yet." } else { "{project.description}" } }
            div { class: "mt-8 grid grid-cols-3 gap-3 border-t border-hairline pt-5 text-sm",
                div { span { class: "block font-semibold text-ink", "0" } span { class: "text-muted", "Sessions" } }
                div { span { class: "block font-semibold text-ink", "0" } span { class: "text-muted", "Docs" } }
                div { span { class: "block font-semibold text-primary", "Open" } span { class: "text-muted", "Project" } }
            }
        }
    }
}
