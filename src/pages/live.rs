use crate::Route;
use crate::backend::models::llm::read_api_key;
use crate::backend::pages::live::LivePageModel;
use crate::components::error::ErrorView;
use crate::state::AppState;
use dioxus::prelude::*;
use tokio::time::{Duration, sleep};

#[derive(Clone, PartialEq)]
struct TranscriptCard {
    time: String,
    text: String,
    has_question: bool,
    confidence: u32,
}

#[derive(Clone, PartialEq)]
struct QaPair {
    question: String,
    answer: String,
    is_thinking: bool,
    confidence: u32,
}

#[component]
pub fn LivePage(project_id: String, session_id: String) -> Element {
    let state = use_context::<AppState>();
    let navigator = use_navigator();
    let mut navigate_to = use_signal(|| None::<Route>);
    let mut action_error = use_signal(|| None::<String>);
    let mut show_no_api_key_dialog = use_signal(|| false);

    use_effect(move || {
        if let Some(route) = navigate_to.take() {
            navigator.push(route);
        }
    });

    let mut is_listening = use_signal(|| false);
    let mut is_thinking = use_signal(|| false);
    let mut thinking_dots = use_signal(|| ".".to_string());

    let mut transcripts = use_signal(Vec::<TranscriptCard>::new);
    let mut draft_transcript = use_signal(|| None::<String>);
    let mut qa_history = use_signal(Vec::<QaPair>::new);

    let mut thinking_generation = use_signal(|| 0u32);

    // Thinking dots animator
    use_effect(move || {
        if is_thinking() {
            let current_gen = thinking_generation();
            thinking_generation += 1;
            spawn(async move {
                loop {
                    sleep(Duration::from_millis(500)).await;
                    if !is_thinking() || thinking_generation() != current_gen + 1 {
                        break;
                    }
                    thinking_dots.with_mut(|d| {
                        if d.len() >= 3 {
                            *d = ".".to_string();
                        } else {
                            d.push('.');
                        }
                    });
                }
            });
        }
    });

    let sid_for_load = session_id.clone();
    let state_for_load = state.clone();
    let mut is_ended = use_signal(|| false);

    let load_history = use_resource(move || {
        let sid = sid_for_load.clone();
        let st = state_for_load.clone();
        async move {
            let model = LivePageModel::new(st);
            model.load(&sid).await
        }
    });

    // Populate history when data is loaded
    use_effect(move || {
        if let Some(Ok(data)) = load_history.read().as_ref() {
            is_ended.set(data.is_ended);

            transcripts.with_mut(|t| {
                t.clear();
                for chunk in &data.transcripts {
                    t.push(TranscriptCard {
                        time: format!("{}s", chunk.start_ms / 1000),
                        text: chunk.text.clone(),
                        has_question: false,
                        confidence: 100,
                    });
                }
            });

            qa_history.with_mut(|h| {
                h.clear();
                for qa in &data.qas {
                    h.push(QaPair {
                        question: qa.query.clone(),
                        answer: qa.answer.clone(),
                        is_thinking: false,
                        confidence: 100,
                    });

                    // Also mark the matching transcript as a question
                    transcripts.with_mut(|t| {
                        if let Some(matched) = t.iter_mut().find(|tr| tr.text == qa.query) {
                            matched.has_question = true;
                        }
                    });
                }
            });
        }
    });

    let state_for_start = state.clone();
    let pid_for_start = project_id.clone();
    let sid_for_start = session_id.clone();

    let on_start = move |_| {
        let has_key = match read_api_key() {
            Ok(key) => !key.is_empty(),
            Err(_) => false,
        };
        if !has_key {
            show_no_api_key_dialog.set(true);
            return;
        }

        let model = LivePageModel::new(state_for_start.clone());
        let pid = pid_for_start.clone();
        let sid = sid_for_start.clone();

        spawn(async move {
            match model.start_recording(pid, sid).await {
                Ok((mut ui_rx, mut segment_rx, mut qa_rx)) => {
                    is_listening.set(true);

                    // UI Chunks listener (for live unstable text)
                    spawn(async move {
                        while let Ok(chunk) = ui_rx.recv().await {
                            if chunk.draft_text.trim().is_empty() {
                                draft_transcript.set(None);
                            } else {
                                draft_transcript.set(Some(chunk.draft_text));
                            }
                        }
                    });

                    // We'll listen to stable segments for the transcript list
                    spawn(async move {
                        while let Ok(segment) = segment_rx.recv().await {
                            draft_transcript.set(None);
                            transcripts.with_mut(|t| {
                                t.push(TranscriptCard {
                                    time: format!("{}s", segment.start_time / 1000),
                                    text: segment.full_text,
                                    has_question: false, // We'll infer this from QA
                                    confidence: 100,
                                });
                            });
                        }
                    });

                    // QA listener
                    spawn(async move {
                        while let Ok(answer) = qa_rx.recv().await {
                            if answer.starts_with("[START_QA]") {
                                let q_text =
                                    if let Some(stripped) = answer.strip_prefix("[START_QA]:") {
                                        stripped.to_string()
                                    } else {
                                        transcripts
                                            .read()
                                            .last()
                                            .map(|last| last.text.clone())
                                            .unwrap_or_default()
                                    };

                                is_thinking.set(true);

                                // Mark the matching transcript internally as a question
                                transcripts.with_mut(|t| {
                                    if let Some(matched) = t.iter_mut().find(|tr| tr.text == q_text)
                                    {
                                        matched.has_question = true;
                                    } else if let Some(last) = t.last_mut() {
                                        last.has_question = true;
                                    }
                                });

                                qa_history.with_mut(|h| {
                                    h.push(QaPair {
                                        question: q_text,
                                        answer: String::new(),
                                        is_thinking: true,
                                        confidence: 100,
                                    });
                                });
                            } else {
                                is_thinking.set(false);
                                if answer.contains("[IGNORE]") {
                                    qa_history.with_mut(|h| {
                                        h.pop(); // Remove the ignored QA completely!
                                    });
                                } else {
                                    qa_history.with_mut(|h| {
                                        if let Some(last) = h.last_mut() {
                                            last.is_thinking = false;
                                            last.answer = answer.clone();
                                        }
                                    });
                                }
                            }
                        }
                    });
                }
                Err(e) => {
                    tracing::error!("Failed to start recording: {e:#}");
                    action_error.set(Some(e.to_string()));
                }
            }
        });
    };

    let state_for_end = state.clone();
    let sid_for_end = session_id.clone();
    let on_end = move |_| {
        let model = LivePageModel::new(state_for_end.clone());
        let sid = sid_for_end.clone();
        spawn(async move {
            if let Err(e) = model.end_session(sid).await {
                tracing::error!("Failed to end session: {e:#}");
                action_error.set(Some(e.to_string()));
            } else {
                is_listening.set(false);
                is_ended.set(true);
            }
        });
    };

    let title = "Live session".to_string();
    let meta = if is_ended() {
        "Session completed (Read Only)".to_string()
    } else if is_listening() {
        "Recording in progress...".to_string()
    } else {
        "Ready to record".to_string()
    };

    rsx! {
        div { class: "flex h-full min-h-0 flex-col overflow-hidden rounded-lg border border-hairline bg-canvas shadow-sm",
            div { class: "flex items-center justify-between border-b border-hairline bg-surface-soft px-4 py-3",
                div { class: "flex items-center gap-3",
                    span { class: if is_listening() { "h-2.5 w-2.5 animate-pulse rounded-full bg-semantic-up" } else { "h-2.5 w-2.5 rounded-full bg-muted" } }
                    h2 { class: "font-semibold text-ink", "{title}" }
                    span { class: "text-sm text-body", "{meta}" }
                }
                div { class: "flex gap-2",
                    if !is_ended() {
                        if !is_listening() {
                            button {
                                class: "rounded-md bg-primary px-4 py-2 text-sm font-semibold text-on-primary hover:bg-primary-active",
                                onclick: on_start,
                                "Start"
                            }
                        } else {
                            button {
                                class: "cursor-not-allowed rounded-md bg-primary-disabled px-4 py-2 text-sm font-semibold text-on-primary",
                                disabled: true,
                                "Start"
                            }
                        }
                        button {
                            class: "rounded-md bg-semantic-down/10 px-4 py-2 text-sm font-semibold text-semantic-down hover:bg-semantic-down/20",
                            onclick: on_end,
                            "End"
                        }
                    }
                }
            }

            if let Some(err) = action_error() {
                div { class: "px-4 pt-4",
                    ErrorView {
                        message: err,
                        on_retry: move |_| action_error.set(None),
                    }
                }
            }

            div { class: "grid min-h-0 flex-1 gap-4 p-4 lg:grid-cols-[minmax(0,3fr)_minmax(280px,1fr)]",
                div { class: "flex min-h-0 flex-col overflow-hidden rounded-lg border border-hairline bg-surface-dark text-on-dark shadow-sm",
                    section { id: "qa-container", class: "flex min-h-0 flex-1 flex-col-reverse gap-6 overflow-auto p-4",
                        if qa_history().is_empty() {
                            div { class: "flex h-full items-center justify-center text-on-dark-soft",
                                "No questions detected yet."
                            }
                        }
                        for pair in qa_history().clone().into_iter().rev() {
                            if pair.is_thinking && pair.answer.is_empty() {
                                div {
                                    key: "thinking-{pair.question}",
                                    class: "flex items-center gap-3 border-b border-surface-dark-elevated pb-6 last:border-b-0 py-4",
                                    div { class: "h-2 w-2 animate-ping rounded-full bg-primary" }
                                    p { class: "font-mono text-sm italic text-on-dark-soft", "Analyzing intent{thinking_dots()}" }
                                }
                            } else {
                                div {
                                    key: "{pair.question}",
                                    class: "flex flex-col gap-4 border-b border-surface-dark-elevated pb-6 last:border-b-0",
                                    div { class: "border-l-4 border-primary bg-surface-dark-elevated px-5 py-4 font-mono text-sm leading-relaxed text-on-dark-soft",
                                        div { class: "mb-3 flex items-center gap-3",
                                            span { class: "text-xs font-semibold uppercase tracking-[0.12em] text-on-dark-soft",
                                                "Detected question"
                                            }
                                        }
                                        p { class: "whitespace-pre-wrap text-base leading-7 text-on-dark",
                                            "{pair.question}"
                                        }
                                    }

                                    div { class: "border-l-4 border-hairline px-5 py-1 font-mono text-sm leading-relaxed text-on-dark-soft",
                                        p { class: "mb-4 text-sm font-semibold text-on-dark-soft",
                                            if pair.is_thinking {
                                                span { class: "italic",
                                                    "Generating response{thinking_dots()}"
                                                }
                                            } else {
                                                span { class: "italic", "Suggested answer" }
                                            }
                                        }
                                        div { class: "whitespace-pre-wrap text-base leading-7",
                                            if pair.answer.is_empty() && pair.is_thinking {
                                                ""
                                            } else {
                                                "{pair.answer}"
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                aside { class: "flex min-h-0 flex-col overflow-hidden rounded-lg border border-hairline bg-surface-soft",
                    div { class: "border-b border-hairline px-3 py-2",
                        h3 { class: "text-sm font-semibold text-ink", "Live transcript" }
                        p { class: "text-xs text-body", "Raw STT chunks" }
                    }
                    div { id: "transcript-container", class: "min-h-0 flex-1 flex flex-col-reverse overflow-auto p-3",
                        if transcripts().is_empty() && draft_transcript().is_none() {
                            div { class: "rounded-md border border-dashed border-hairline p-4 text-center text-sm text-body",
                                "No transcript yet."
                            }
                        }
                        if let Some(draft) = draft_transcript() {
                            div { class: "mb-3 rounded-md bg-canvas p-3 shadow-sm opacity-60 animate-pulse",
                                div { class: "mb-1 flex items-center justify-between gap-2",
                                    span { class: "truncate text-xs font-semibold uppercase tracking-[0.08em] text-muted",
                                        "Draft"
                                    }
                                }
                                p { class: "text-sm leading-relaxed text-body italic",
                                    "{draft}"
                                }
                            }
                        }
                        for chunk in transcripts().clone().into_iter().rev() {
                            TranscriptChunkView {
                                key: "{chunk.time}_{chunk.text}",
                                chunk,
                            }
                        }
                    }
                }
            }
        }

        if show_no_api_key_dialog() {
            div { class: "fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-sm",
                div { class: "w-full max-w-sm rounded-lg bg-canvas p-6 shadow-xl border border-hairline text-center",
                    h2 { class: "text-lg font-bold text-ink mb-2", "No API Key Configured" }
                    p { class: "text-sm text-body mb-6",
                        "You must configure an OpenAI API Key in the Settings page before starting a live session."
                    }
                    div { class: "flex justify-center gap-3",
                        button {
                            class: "rounded-md px-4 py-2 text-sm font-semibold text-body hover:bg-surface-strong border border-hairline",
                            onclick: move |_| show_no_api_key_dialog.set(false),
                            "Close"
                        }
                        button {
                            class: "rounded-md bg-primary px-4 py-2 text-sm font-semibold text-on-primary hover:bg-primary-active",
                            onclick: move |_| {
                                show_no_api_key_dialog.set(false);
                                navigator.push(Route::SettingsPage {});
                            },
                            "Go to Settings"
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn TranscriptChunkView(chunk: TranscriptCard) -> Element {
    rsx! {
        div { class: "mb-3 rounded-md bg-canvas p-3 shadow-sm",
            div { class: "mb-1 flex items-center justify-between gap-2",
                span { class: "truncate text-xs font-semibold uppercase tracking-[0.08em] text-primary",
                    "Transcript"
                }
                span { class: "font-mono text-[10px] text-muted", "{chunk.time}" }
            }
            p { class: "text-sm leading-relaxed text-body", "{chunk.text}" }
        }
    }
}
