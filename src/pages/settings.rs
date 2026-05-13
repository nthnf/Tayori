use dioxus::prelude::*;

use crate::types::{SettingsForm, Theme};

#[component]
pub fn SettingsPage(
    mut theme: Signal<Theme>,
    mut settings: Signal<SettingsForm>,
    saved_settings: Signal<SettingsForm>,
    on_save: EventHandler<SettingsForm>,
) -> Element {
    let has_changes = settings() != saved_settings();

    let save_changes = move |_| on_save.call(settings());

    let reset_changes = move |_| {
        let saved = saved_settings();
        theme.set(saved.ui_theme);
        settings.set(saved);
    };

    rsx! {
        div { class: "flex h-full flex-col overflow-hidden rounded-lg border border-hairline bg-canvas shadow-sm",
            div { class: "flex items-center justify-between gap-4 border-b border-hairline bg-surface-soft px-4 py-3",
                div {
                    h2 { class: "text-lg font-semibold text-ink", "Settings" }
                     p { class: "text-sm text-body", "Edit runtime defaults. Provider fields are stored locally in SQLite." }
                }
                div { class: "flex gap-2",
                    button { class: "rounded-md px-4 py-2 text-sm font-semibold text-body hover:bg-surface-strong", disabled: !has_changes, onclick: reset_changes, "Reset" }
                    button { class: if has_changes { "rounded-md bg-primary px-4 py-2 text-sm font-semibold text-on-primary hover:bg-primary-active" } else { "cursor-not-allowed rounded-md bg-primary-disabled px-4 py-2 text-sm font-semibold text-on-primary" }, disabled: !has_changes, onclick: save_changes, "Save" }
                }
            }

            div { class: "min-h-0 flex-1 overflow-auto divide-y divide-hairline px-4",
                ToggleRow {
                    name: "Dark mode".to_string(),
                    description: "Switch the app shell between light and dark UI tokens.".to_string(),
                    enabled: theme() == Theme::Dark,
                    onclick: move |_| {
                        let next = if theme() == Theme::Dark { Theme::Light } else { Theme::Dark };
                        theme.set(next);
                        settings.write().ui_theme = next;
                    }
                }
                ReadOnlyRow { name: "LLM provider".to_string(), description: "Provider used for suggested answers and summaries.".to_string(), value: settings.read().llm_provider.clone() }
                TextRow { name: "LLM model".to_string(), description: "Model name used by the selected LLM provider.".to_string(), value: settings.read().llm_model.clone(), placeholder: "gpt-4.1-mini".to_string(), oninput: move |value| settings.write().llm_model = value }
                ReadOnlyRow { name: "Embedding provider".to_string(), description: "Embedding runtime used for document chunks and transcript summaries.".to_string(), value: settings.read().embedding_provider.clone() }
                TextRow { name: "Embedding model".to_string(), description: "Model used to embed document chunks and summaries.".to_string(), value: settings.read().embedding_model.clone(), placeholder: "jinaai/jina-embeddings-v2-base-code".to_string(), oninput: move |value| settings.write().embedding_model = value }
                TextRow { name: "Sparse model".to_string(), description: "Model used to create sparse index/value vectors for hybrid retrieval.".to_string(), value: settings.read().sparse_model.clone(), placeholder: "prithivida/splade_pp_en_v1".to_string(), oninput: move |value| settings.write().sparse_model = value }
                TextRow { name: "Reranker model".to_string(), description: "Model used to re-rank retrieved document and transcript matches.".to_string(), value: settings.read().reranker_model.clone(), placeholder: "jinaai/jina-reranker-v1-turbo-en".to_string(), oninput: move |value| settings.write().reranker_model = value }
                TextRow { name: "Whisper model".to_string(), description: "Local speech-to-text model name.".to_string(), value: settings.read().whisper_model.clone(), placeholder: "small-q8_0".to_string(), oninput: move |value| settings.write().whisper_model = value }
                TextRow { name: "Summary window".to_string(), description: "Minutes of transcript chunks to combine into each summary. Valid range is 1 to 10.".to_string(), value: settings.read().summary_minutes.clone(), placeholder: "5".to_string(), oninput: move |value| settings.write().summary_minutes = value }
            }
        }
    }
}

#[component]
fn TextRow(
    name: String,
    description: String,
    value: String,
    placeholder: String,
    oninput: EventHandler<String>,
) -> Element {
    rsx! {
        div { class: "grid gap-3 py-4 md:grid-cols-[minmax(220px,1fr)_minmax(280px,420px)] md:items-center",
            div {
                p { class: "font-semibold text-ink", "{name}" }
                p { class: "mt-1 text-sm text-body", "{description}" }
            }
            input { class: "rounded-md border border-hairline bg-canvas px-3 py-2 text-ink outline-none focus:border-primary", value, placeholder, oninput: move |event| oninput.call(event.value()) }
        }
    }
}

#[component]
fn ReadOnlyRow(name: String, description: String, value: String) -> Element {
    rsx! {
        div { class: "grid gap-3 py-4 md:grid-cols-[minmax(220px,1fr)_minmax(280px,420px)] md:items-center",
            div {
                p { class: "font-semibold text-ink", "{name}" }
                p { class: "mt-1 text-sm text-body", "{description}" }
            }
            div { class: "rounded-md border border-hairline bg-surface-soft px-3 py-2 text-body", "{value}" }
        }
    }
}

#[component]
fn ToggleRow(
    name: String,
    description: String,
    enabled: bool,
    onclick: EventHandler<MouseEvent>,
) -> Element {
    rsx! {
        div { class: "grid gap-3 py-4 md:grid-cols-[minmax(220px,1fr)_minmax(280px,420px)] md:items-center",
            div {
                p { class: "font-semibold text-ink", "{name}" }
                p { class: "mt-1 text-sm text-body", "{description}" }
            }
            button { class: if enabled { "flex h-9 w-16 items-center justify-end rounded-full bg-primary p-1" } else { "flex h-9 w-16 items-center justify-start rounded-full bg-surface-strong p-1" }, onclick: move |event| onclick.call(event),
                span { class: "h-7 w-7 rounded-full bg-on-primary shadow-sm" }
            }
        }
    }
}
