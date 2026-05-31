use crate::Route;
use dioxus::prelude::*;

/// A reusable error view component that displays an error message
/// with a retry button — similar to how web apps show error pages.
#[component]
pub fn ErrorView(message: String, #[props(default)] on_retry: Option<EventHandler<()>>) -> Element {
    let navigator = use_navigator();

    rsx! {
        div { class: "flex h-full w-full items-center justify-center bg-canvas p-8",
            div { class: "flex max-w-lg flex-col items-center text-center",
                // Icon
                div { class: "mb-6 grid h-16 w-16 place-items-center rounded-full bg-semantic-down/10 text-3xl text-semantic-down",
                    "!"
                }

                h2 { class: "mb-2 text-xl font-bold text-ink", "Something went wrong" }

                p { class: "mb-8 text-sm leading-relaxed text-body", "{message}" }

                // Error details (code block)
                div { class: "mb-8 w-full rounded-lg border border-hairline bg-surface-soft p-4 text-left",
                    p { class: "mb-1 text-xs font-semibold uppercase tracking-[0.08em] text-muted",
                        "Error details"
                    }
                    pre { class: "whitespace-pre-wrap break-words font-mono text-xs text-semantic-down",
                        "{message}"
                    }
                }

                div { class: "flex gap-3",
                    if let Some(handler) = &on_retry {
                        button {
                            class: "rounded-full bg-primary px-6 py-2.5 text-sm font-semibold text-on-primary hover:bg-primary-active",
                            onclick: {
                                let handler = *handler;
                                move |_| handler.call(())
                            },
                            "Try again"
                        }
                    }
                    button {
                        class: "rounded-full border border-hairline px-6 py-2.5 text-sm font-semibold text-body hover:bg-surface-strong hover:text-ink",
                        onclick: move |_| {
                            navigator.push(Route::DashboardPage);
                        },
                        "Go to Dashboard"
                    }
                }
            }
        }
    }
}
