use dioxus::prelude::*;

use crate::types::{SessionCard, TranscriptCard};

#[component]
pub fn LiveRecordingPage(
    session: Option<SessionCard>,
    transcripts: Signal<Vec<TranscriptCard>>,
    is_listening: bool,
    on_start: EventHandler<MouseEvent>,
    on_pause: EventHandler<MouseEvent>,
    on_end: EventHandler<MouseEvent>,
) -> Element {
    let title = session
        .as_ref()
        .map(|session| session.title.clone())
        .unwrap_or_else(|| "Live session".to_string());
    let meta = session
        .as_ref()
        .map(|session| session.meta.clone())
        .unwrap_or_else(|| "No active session selected".to_string());
    let messages = transcripts.read().clone();

    rsx! {
        div { class: "flex h-full min-h-0 flex-col overflow-hidden rounded-lg border border-hairline bg-canvas shadow-sm",
            div { class: "flex items-center justify-between border-b border-hairline bg-surface-soft px-4 py-3",
                div { class: "flex items-center gap-3",
                    span { class: if is_listening { "h-2.5 w-2.5 animate-pulse rounded-full bg-semantic-up" } else { "h-2.5 w-2.5 rounded-full bg-muted" } }
                    h2 { class: "font-semibold text-ink", "{title}" }
                    span { class: "text-sm text-body", "{meta}" }
                }
                div { class: "flex gap-2",
                    button { class: if is_listening { "cursor-not-allowed rounded-md bg-primary-disabled px-4 py-2 text-sm font-semibold text-on-primary" } else { "rounded-md bg-primary px-4 py-2 text-sm font-semibold text-on-primary hover:bg-primary-active" }, disabled: is_listening, onclick: move |event| on_start.call(event), "Start" }
                    button { class: if is_listening { "rounded-md bg-surface-strong px-4 py-2 text-sm font-semibold text-ink hover:bg-hairline" } else { "cursor-not-allowed rounded-md bg-surface-strong/60 px-4 py-2 text-sm font-semibold text-muted" }, disabled: !is_listening, onclick: move |event| on_pause.call(event), "Pause" }
                    button { class: "rounded-md bg-semantic-down/10 px-4 py-2 text-sm font-semibold text-semantic-down hover:bg-semantic-down/20", onclick: move |event| on_end.call(event), "End" }
                }
            }

            div { class: "min-h-0 flex-1 p-4",
                section { class: "flex h-full min-h-0 flex-col overflow-hidden rounded-lg border border-hairline bg-surface-soft",
                    div { class: "border-b border-hairline px-4 py-3",
                        h3 { class: "font-semibold text-ink", "Conversation" }
                        p { class: "text-sm text-body", "Transcript chunks and assistant responses will appear here as one scrollable thread." }
                    }
                    div { class: "min-h-0 flex-1 overflow-auto p-4",
                        if messages.is_empty() {
                            div { class: "grid h-full place-items-center text-center text-body",
                                div {
                                    h4 { class: "font-semibold text-ink", "No transcript yet" }
                                    p { class: "mt-2 max-w-md text-sm", "Start recording to stream speech-to-text chunks into this session. Detected questions and answers will use this same conversation thread." }
                                }
                            }
                        }
                        for message in messages {
                            TranscriptMessage { message }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn TranscriptMessage(message: TranscriptCard) -> Element {
    rsx! {
        div { class: "mb-3 max-w-3xl rounded-lg bg-canvas p-4 shadow-sm",
            div { class: "mb-1 flex items-center justify-between gap-2",
                span { class: "truncate text-xs font-semibold uppercase tracking-[0.08em] text-primary", "Transcript" }
                span { class: "font-mono text-[10px] text-muted", "{message.time}" }
            }
            p { class: "text-sm leading-relaxed text-body", "{message.text}" }
        }
    }
}
