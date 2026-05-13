use dioxus::prelude::*;

use crate::types::{DocumentCard, ProjectCard, SessionCard};

#[component]
pub fn ProjectDetailPage(
    project: Option<ProjectCard>,
    documents: Signal<Vec<DocumentCard>>,
    sessions: Signal<Vec<SessionCard>>,
    on_upload_document: EventHandler<MouseEvent>,
    on_open_session: EventHandler<String>,
    on_create_session: EventHandler<MouseEvent>,
) -> Element {
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
                }

                div { class: "grid min-h-0 flex-1 grid-rows-[210px_minmax(0,1fr)] gap-4 p-4",
                    button { class: "grid place-items-center rounded-lg border border-dashed border-hairline bg-surface-soft p-6 text-center hover:border-primary hover:bg-canvas", onclick: move |event| on_upload_document.call(event),
                        div {
                            div { class: "mx-auto mb-4 grid h-12 w-12 place-items-center rounded-md bg-canvas text-xl text-primary", "+" }
                            h3 { class: "font-semibold text-ink", "Upload documents" }
                            p { class: "mt-2 max-w-md text-sm text-body", "Choose markdown, text, CSV, or JSON files. Tayori stores metadata, chunks text, and indexes searchable context." }
                        }
                    }

                    div { class: "min-h-0 overflow-hidden rounded-lg border border-hairline",
                        div { class: "grid grid-cols-[1fr_140px_110px] gap-3 border-b border-hairline bg-surface-soft px-4 py-2 text-xs font-semibold uppercase tracking-[0.08em] text-muted",
                            span { "File" }
                            span { "Type" }
                            span { "Status" }
                        }
                        div { class: "max-h-full overflow-auto divide-y divide-hairline",
                            if documents.read().is_empty() {
                                div { class: "p-6 text-center text-sm text-body", "No documents uploaded yet." }
                            }
                            for document in documents.read().clone() {
                                DocumentRow { document }
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
                    button { class: "rounded-md bg-primary px-3 py-2 text-sm font-semibold text-on-primary hover:bg-primary-active", onclick: move |event| on_create_session.call(event), "Create" }
                }
                div { class: "min-h-0 flex-1 overflow-auto p-3",
                    if sessions.read().is_empty() {
                        div { class: "p-4 text-center text-sm text-body", "No sessions yet." }
                    }
                    for session in sessions.read().clone() {
                        SessionRow { session, onclick: move |session_id| on_open_session.call(session_id) }
                    }
                }
            }
        }
    }
}

#[component]
fn DocumentRow(document: DocumentCard) -> Element {
    rsx! {
        div { class: "grid grid-cols-[1fr_140px_110px] gap-3 px-4 py-3 hover:bg-surface-soft",
            div {
                p { class: "font-semibold text-ink", "{document.name}" }
                p { class: "text-sm text-body", "{document.meta}" }
            }
            span { class: "self-center text-sm text-body", "{document.kind}" }
            span { class: "self-center rounded-full bg-surface-strong px-3 py-1 text-center text-xs font-semibold text-ink", "{document.status}" }
        }
    }
}

#[component]
fn SessionRow(session: SessionCard, onclick: EventHandler<String>) -> Element {
    let session_id = session.id.clone();

    rsx! {
        button { class: "mb-2 w-full rounded-md px-3 py-3 text-left hover:bg-surface-soft", onclick: move |_| onclick.call(session_id.clone()),
            p { class: "font-semibold text-ink", "{session.title}" }
            p { class: "mt-1 text-sm text-body", "{session.meta}" }
        }
    }
}
