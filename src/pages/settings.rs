use dioxus::prelude::*;

use crate::types::Theme;

#[component]
pub fn SettingsPage(mut theme: Signal<Theme>) -> Element {
    let mut saved_theme = use_signal(|| Theme::Light);
    let mut llm_model = use_signal(|| "gpt-4.1-mini".to_string());
    let mut saved_llm_model = use_signal(|| "gpt-4.1-mini".to_string());
    let mut api_key_ref = use_signal(|| "system keyring ref".to_string());
    let mut saved_api_key_ref = use_signal(|| "system keyring ref".to_string());
    let mut store_responses = use_signal(|| false);
    let mut saved_store_responses = use_signal(|| false);
    let mut embedding_model = use_signal(|| "jinaai/jina-embeddings-v2-base-code".to_string());
    let mut saved_embedding_model =
        use_signal(|| "jinaai/jina-embeddings-v2-base-code".to_string());
    let mut sparse_model = use_signal(|| "prithivida/splade_pp_en_v1".to_string());
    let mut saved_sparse_model = use_signal(|| "prithivida/splade_pp_en_v1".to_string());
    let mut reranker_model = use_signal(|| "jinaai/jina-reranker-v1-turbo-en".to_string());
    let mut saved_reranker_model = use_signal(|| "jinaai/jina-reranker-v1-turbo-en".to_string());
    let mut whisper_model = use_signal(|| "default local model".to_string());
    let mut saved_whisper_model = use_signal(|| "default local model".to_string());
    let mut summary_minutes = use_signal(|| "5".to_string());
    let mut saved_summary_minutes = use_signal(|| "5".to_string());

    let has_changes = theme() != saved_theme()
        || llm_model() != saved_llm_model()
        || api_key_ref() != saved_api_key_ref()
        || store_responses() != saved_store_responses()
        || embedding_model() != saved_embedding_model()
        || sparse_model() != saved_sparse_model()
        || reranker_model() != saved_reranker_model()
        || whisper_model() != saved_whisper_model()
        || summary_minutes() != saved_summary_minutes();

    let save_changes = move |_| {
        saved_theme.set(theme());
        saved_llm_model.set(llm_model());
        saved_api_key_ref.set(api_key_ref());
        saved_store_responses.set(store_responses());
        saved_embedding_model.set(embedding_model());
        saved_sparse_model.set(sparse_model());
        saved_reranker_model.set(reranker_model());
        saved_whisper_model.set(whisper_model());
        saved_summary_minutes.set(summary_minutes());
    };

    let reset_changes = move |_| {
        theme.set(saved_theme());
        llm_model.set(saved_llm_model());
        api_key_ref.set(saved_api_key_ref());
        store_responses.set(saved_store_responses());
        embedding_model.set(saved_embedding_model());
        sparse_model.set(saved_sparse_model());
        reranker_model.set(saved_reranker_model());
        whisper_model.set(saved_whisper_model());
        summary_minutes.set(saved_summary_minutes());
    };

    rsx! {
        div { class: "flex h-full flex-col overflow-hidden rounded-lg border border-hairline bg-canvas shadow-sm",
            div { class: "flex items-center justify-between gap-4 border-b border-hairline bg-surface-soft px-4 py-3",
                div {
                    h2 { class: "text-lg font-semibold text-ink", "Settings" }
                    p { class: "text-sm text-body", "Edit runtime defaults. Provider fields are locked until backend wiring is ready." }
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
                        if theme() == Theme::Dark { theme.set(Theme::Light) } else { theme.set(Theme::Dark) }
                    }
                }
                ReadOnlyRow { name: "LLM provider".to_string(), description: "Provider used for suggested answers and summaries. Locked for now.".to_string(), value: "OpenAI-compatible".to_string() }
                TextRow { name: "LLM model".to_string(), description: "Model name used by the selected LLM provider.".to_string(), value: llm_model, placeholder: "gpt-4.1-mini".to_string() }
                TextRow { name: "API key reference".to_string(), description: "Reference to the local secret or keyring entry, not the raw API key.".to_string(), value: api_key_ref, placeholder: "system keyring ref".to_string() }
                ToggleRow { name: "Store LLM responses".to_string(), description: "Persist generated answers for later review. Off by default.".to_string(), enabled: store_responses(), onclick: move |_| store_responses.set(!store_responses()) }
                ReadOnlyRow { name: "Embedding provider".to_string(), description: "Embedding runtime used for document chunks and transcript summaries. Locked for now.".to_string(), value: "FastEmbed".to_string() }
                TextRow { name: "Embedding model".to_string(), description: "Model used to embed document chunks and summaries.".to_string(), value: embedding_model, placeholder: "jinaai/jina-embeddings-v2-base-code".to_string() }
                TextRow { name: "Sparse model".to_string(), description: "Model used to create sparse index/value vectors for hybrid retrieval.".to_string(), value: sparse_model, placeholder: "prithivida/splade_pp_en_v1".to_string() }
                TextRow { name: "Reranker model".to_string(), description: "Model used to re-rank retrieved document and transcript matches.".to_string(), value: reranker_model, placeholder: "jinaai/jina-reranker-v1-turbo-en".to_string() }
                TextRow { name: "Whisper model".to_string(), description: "Local speech-to-text model path or configured default.".to_string(), value: whisper_model, placeholder: "default local model".to_string() }
                TextRow { name: "Summary window".to_string(), description: "Minutes of transcript chunks to combine into each summary. Valid range is 1 to 10.".to_string(), value: summary_minutes, placeholder: "5".to_string() }
            }
        }
    }
}

#[component]
fn TextRow(
    name: String,
    description: String,
    mut value: Signal<String>,
    placeholder: String,
) -> Element {
    rsx! {
        div { class: "grid gap-3 py-4 md:grid-cols-[minmax(220px,1fr)_minmax(280px,420px)] md:items-center",
            div {
                p { class: "font-semibold text-ink", "{name}" }
                p { class: "mt-1 text-sm text-body", "{description}" }
            }
            input { class: "rounded-md border border-hairline bg-canvas px-3 py-2 text-ink outline-none focus:border-primary", value: "{value}", placeholder, oninput: move |event| value.set(event.value()) }
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
