use dioxus::prelude::*;

use crate::types::{SessionCard, TranscriptCard};

#[component]
pub fn LiveRecordingPage(
    session: Option<SessionCard>,
    transcripts: Signal<Vec<TranscriptCard>>,
    detected_question: Option<String>,
    suggested_answer: Option<String>,
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
    let is_ended = session
        .as_ref()
        .map(|session| session.status == "completed")
        .unwrap_or(false);
    let transcript_chunks = transcripts.read().clone();
    let question = detected_question.unwrap_or_else(|| "No question detected yet.".to_string());
    let answer = suggested_answer.unwrap_or_else(|| {
        "Suggested answer will appear after Tayori detects a question and calls the configured LLM.".to_string()
    });

    rsx! {
        div { class: "flex h-full min-h-0 flex-col overflow-hidden rounded-lg border border-hairline bg-canvas shadow-sm",
            div { class: "flex items-center justify-between border-b border-hairline bg-surface-soft px-4 py-3",
                div { class: "flex items-center gap-3",
                    span { class: if is_listening { "h-2.5 w-2.5 animate-pulse rounded-full bg-semantic-up" } else { "h-2.5 w-2.5 rounded-full bg-muted" } }
                    h2 { class: "font-semibold text-ink", "{title}" }
                    span { class: "text-sm text-body", "{meta}" }
                }
                div { class: "flex gap-2",
                    if !is_ended {
                        button { class: if is_listening { "cursor-not-allowed rounded-md bg-primary-disabled px-4 py-2 text-sm font-semibold text-on-primary" } else { "rounded-md bg-primary px-4 py-2 text-sm font-semibold text-on-primary hover:bg-primary-active" }, disabled: is_listening, onclick: move |event| on_start.call(event), "Start" }
                        button { class: if is_listening { "rounded-md bg-surface-strong px-4 py-2 text-sm font-semibold text-ink hover:bg-hairline" } else { "cursor-not-allowed rounded-md bg-surface-strong/60 px-4 py-2 text-sm font-semibold text-muted" }, disabled: !is_listening, onclick: move |event| on_pause.call(event), "Pause" }
                        button { class: "rounded-md bg-semantic-down/10 px-4 py-2 text-sm font-semibold text-semantic-down hover:bg-semantic-down/20", onclick: move |event| on_end.call(event), "End" }
                    }
                }
            }

            div { class: "grid min-h-0 flex-1 gap-4 p-4 lg:grid-cols-[minmax(0,4fr)_minmax(220px,1fr)]",
                section { class: "grid min-h-0 grid-rows-[auto_minmax(0,1fr)] gap-4 overflow-hidden",
                    div { class: "rounded-lg border border-hairline bg-surface-soft p-5",
                        div { class: "mb-3 flex items-center justify-between gap-3",
                            p { class: "text-xs font-semibold uppercase tracking-[0.08em] text-muted", "Detected question" }
                            if let Some(latest) = transcript_chunks.iter().rev().find(|chunk| chunk.has_question) {
                                span { class: "rounded-full bg-canvas px-3 py-1 text-xs font-semibold text-primary", "Confidence {latest.confidence}%" }
                            }
                        }
                        h3 { class: "text-2xl font-semibold tracking-[-0.02em] text-ink", "{question}" }
                    }

                    div { class: "min-h-0 overflow-auto rounded-lg border border-hairline bg-canvas p-5",
                        div { class: "mb-4 border-b border-hairline pb-3",
                            h3 { class: "font-semibold text-ink", "Suggested answer" }
                            p { class: "text-sm text-body", "Generated from project context and the latest detected question." }
                        }
                        div { class: "whitespace-pre-wrap text-sm leading-relaxed text-body", "{answer}" }
                    }
                }

                aside { class: "flex min-h-0 flex-col overflow-hidden rounded-lg border border-hairline bg-surface-soft",
                    div { class: "border-b border-hairline px-3 py-2",
                        h3 { class: "text-sm font-semibold text-ink", "Live transcript" }
                        p { class: "text-xs text-body", "Raw STT chunks" }
                    }
                    div { class: "min-h-0 flex-1 overflow-auto p-3",
                        if transcript_chunks.is_empty() {
                            div { class: "rounded-md border border-dashed border-hairline p-4 text-center text-sm text-body", "No transcript yet." }
                        }
                        for chunk in transcript_chunks {
                            TranscriptChunk { chunk }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn TranscriptChunk(chunk: TranscriptCard) -> Element {
    let label = if chunk.has_question {
        "Question"
    } else {
        "Transcript"
    };

    rsx! {
        div { class: if chunk.has_question { "mb-3 rounded-md border border-primary/30 bg-canvas p-3 shadow-sm" } else { "mb-3 rounded-md bg-canvas p-3 shadow-sm" },
            div { class: "mb-1 flex items-center justify-between gap-2",
                span { class: "truncate text-xs font-semibold uppercase tracking-[0.08em] text-primary", "{label}" }
                span { class: "font-mono text-[10px] text-muted", "{chunk.time}" }
            }
            p { class: "text-sm leading-relaxed text-body", "{chunk.text}" }
        }
    }
}
