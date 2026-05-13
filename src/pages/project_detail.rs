use dioxus::prelude::*;

use crate::types::{Page, ProjectCard};

#[component]
pub fn ProjectDetailPage(project: Option<ProjectCard>, mut page: Signal<Page>) -> Element {
    let project_name = project
        .as_ref()
        .map(|project| project.name.clone())
        .unwrap_or_else(|| "Untitled project".to_string());

    rsx! {
        div { class: "grid h-full min-h-0 gap-4 lg:grid-cols-[minmax(0,1fr)_340px]",
            section { class: "flex min-h-0 flex-col overflow-hidden rounded-lg border border-hairline bg-canvas shadow-sm",
                div { class: "flex items-center justify-between border-b border-hairline bg-surface-soft px-4 py-3",
                    div {
                        h2 { class: "text-lg font-semibold text-ink", "{project_name}" }
                        p { class: "text-sm text-body", "Documents and metadata" }
                    }
                    button { class: "rounded-md bg-primary px-4 py-2 text-sm font-semibold text-on-primary hover:bg-primary-active", "Upload Docs" }
                }

                div { class: "grid min-h-0 flex-1 grid-rows-[210px_minmax(0,1fr)] gap-4 p-4",
                    div { class: "grid place-items-center rounded-lg border border-dashed border-hairline bg-surface-soft p-6 text-center",
                        div {
                            div { class: "mx-auto mb-4 grid h-12 w-12 place-items-center rounded-md bg-canvas text-xl text-primary", "+" }
                            h3 { class: "font-semibold text-ink", "Upload documents" }
                            p { class: "mt-2 max-w-md text-sm text-body", "Drop PDFs, markdown, notes, or transcripts here. Tayori will store metadata, chunk text, and index searchable context." }
                        }
                    }

                    div { class: "min-h-0 overflow-hidden rounded-lg border border-hairline",
                        div { class: "grid grid-cols-[1fr_140px_110px] gap-3 border-b border-hairline bg-surface-soft px-4 py-2 text-xs font-semibold uppercase tracking-[0.08em] text-muted",
                            span { "File" }
                            span { "Type" }
                            span { "Status" }
                        }
                        div { class: "max-h-full overflow-auto divide-y divide-hairline",
                            DocumentRow { name: "Compliance_Report_V2.pdf".to_string(), meta: "2.4 MB • Added today".to_string(), kind: "PDF".to_string() }
                            DocumentRow { name: "Kickoff transcript.md".to_string(), meta: "18 KB • Added today".to_string(), kind: "Markdown".to_string() }
                            DocumentRow { name: "Research notes.txt".to_string(), meta: "9 KB • Added today".to_string(), kind: "Text".to_string() }
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
                    button { class: "rounded-md bg-primary px-3 py-2 text-sm font-semibold text-on-primary hover:bg-primary-active", onclick: move |_| page.set(Page::LiveRecording), "Create" }
                }
                div { class: "min-h-0 flex-1 overflow-auto p-3",
                    SessionRow { title: "Design review".to_string(), meta: "Today • 42 min • Draft".to_string(), onclick: move |_| page.set(Page::LiveRecording) }
                    SessionRow { title: "Client kickoff".to_string(), meta: "Yesterday • 31 min • Indexed".to_string(), onclick: move |_| page.set(Page::LiveRecording) }
                    SessionRow { title: "Research sync".to_string(), meta: "May 10 • 55 min • Indexed".to_string(), onclick: move |_| page.set(Page::LiveRecording) }
                }
            }
        }
    }
}

#[component]
fn DocumentRow(name: String, meta: String, kind: String) -> Element {
    rsx! {
        div { class: "grid grid-cols-[1fr_140px_110px] gap-3 px-4 py-3 hover:bg-surface-soft",
            div {
                p { class: "font-semibold text-ink", "{name}" }
                p { class: "text-sm text-body", "{meta}" }
            }
            span { class: "self-center text-sm text-body", "{kind}" }
            span { class: "self-center rounded-full bg-surface-strong px-3 py-1 text-center text-xs font-semibold text-ink", "Ready" }
        }
    }
}

#[component]
fn SessionRow(title: String, meta: String, onclick: EventHandler<MouseEvent>) -> Element {
    rsx! {
        button { class: "mb-2 w-full rounded-md px-3 py-3 text-left hover:bg-surface-soft", onclick: move |event| onclick.call(event),
            p { class: "font-semibold text-ink", "{title}" }
            p { class: "mt-1 text-sm text-body", "{meta}" }
        }
    }
}
