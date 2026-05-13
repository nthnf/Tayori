use dioxus::prelude::*;

#[component]
pub fn LiveRecordingPage() -> Element {
    rsx! {
        div { class: "flex h-full min-h-0 flex-col overflow-hidden rounded-lg border border-hairline bg-canvas shadow-sm",
            div { class: "flex items-center justify-between border-b border-hairline bg-surface-soft px-4 py-3",
                div { class: "flex items-center gap-3",
                    span { class: "h-2.5 w-2.5 animate-pulse rounded-full bg-semantic-up" }
                    h2 { class: "font-semibold text-ink", "Live Session" }
                    span { class: "font-mono text-sm text-body", "00:42:15" }
                }
                div { class: "flex gap-2",
                    button { class: "rounded-md bg-primary px-4 py-2 text-sm font-semibold text-on-primary hover:bg-primary-active", "Start" }
                    button { class: "rounded-md bg-surface-strong px-4 py-2 text-sm font-semibold text-ink hover:bg-hairline", "Pause" }
                    button { class: "rounded-md bg-semantic-down/10 px-4 py-2 text-sm font-semibold text-semantic-down hover:bg-semantic-down/20", "End" }
                }
            }

            div { class: "grid min-h-0 flex-1 gap-4 p-4 lg:grid-cols-[minmax(0,4fr)_minmax(220px,1fr)]",
                section { class: "grid min-h-0 grid-rows-[auto_minmax(0,1fr)] gap-4 overflow-hidden",
                    div { class: "rounded-lg border border-hairline bg-surface-soft p-5",
                        p { class: "text-xs font-semibold uppercase tracking-[0.08em] text-muted", "Detected question" }
                        h3 { class: "mt-3 text-2xl font-semibold tracking-[-0.02em] text-ink", "How should the desktop dashboard organize projects, documents, and live sessions without feeling like a web app?" }
                        div { class: "mt-4 flex gap-2 text-xs font-semibold uppercase tracking-[0.08em]",
                            span { class: "rounded-full bg-canvas px-3 py-1 text-primary", "Confidence 91%" }
                            span { class: "rounded-full bg-canvas px-3 py-1 text-body", "Design review" }
                        }
                    }

                    div { class: "min-h-0 overflow-auto rounded-lg border border-hairline bg-canvas p-5",
                        div { class: "mb-4 flex items-center justify-between border-b border-hairline pb-3",
                            h3 { class: "font-semibold text-ink", "Suggested LLM Answer" }
                            button { class: "rounded-md bg-surface-strong px-3 py-1.5 text-sm font-semibold text-ink", "Copy" }
                        }
                        div { class: "prose max-w-none text-body",
                            p { "Use a native desktop shell pattern: a compact title bar, one primary page title, and dense panes instead of marketing-style cards. The dashboard should behave like a project manager, with sortable project rows and a modal for creation." }
                            p { "Inside a project, split the workspace vertically. Keep documents and upload state on the large left pane because this is the source material. Keep sessions on the narrower right pane because it is a navigational list and action surface." }
                            p { "For live recording, dedicate roughly 80% of width to the active assistant surface: detected question, suggested answer, and controls. Reserve the remaining 20% for the raw transcript stream so it stays visible without competing with the answer." }
                        }
                    }
                }

                aside { class: "flex min-h-0 flex-col overflow-hidden rounded-lg border border-hairline bg-surface-soft",
                    div { class: "border-b border-hairline px-3 py-2",
                        h3 { class: "text-sm font-semibold text-ink", "Detected Transcript" }
                        p { class: "text-xs text-body", "Live STT chunks" }
                    }
                    div { class: "min-h-0 flex-1 overflow-auto p-3",
                        TranscriptChunk { speaker: "Interviewer".to_string(), time: "10:02".to_string(), text: "Can you explain how this should feel more like a desktop app?".to_string() }
                        TranscriptChunk { speaker: "Candidate".to_string(), time: "10:03".to_string(), text: "The top navigation should be closer to Godot, with compact controls and a central page title.".to_string() }
                        TranscriptChunk { speaker: "Interviewer".to_string(), time: "10:04".to_string(), text: "What should the project page contain?".to_string() }
                        TranscriptChunk { speaker: "Candidate".to_string(), time: "10:05".to_string(), text: "A document pane and a sessions pane, with create session routing into the recorder.".to_string() }
                    }
                }
            }
        }
    }
}

#[component]
fn TranscriptChunk(speaker: String, time: String, text: String) -> Element {
    rsx! {
        div { class: "mb-3 rounded-md bg-canvas p-3 shadow-sm",
            div { class: "mb-1 flex items-center justify-between gap-2",
                span { class: "truncate text-xs font-semibold uppercase tracking-[0.08em] text-primary", "{speaker}" }
                span { class: "font-mono text-[10px] text-muted", "{time}" }
            }
            p { class: "text-sm leading-relaxed text-body", "{text}" }
        }
    }
}
