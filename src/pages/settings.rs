use crate::backend::models::llm::{delete_api_key, read_api_key, store_api_key};
use crate::backend::pages::settings::SettingsPageModel;
use crate::components::error::ErrorView;
use crate::state::AppState;
use dioxus::prelude::*;

#[component]
pub fn SettingsPage() -> Element {
    let state = use_context::<AppState>();

    // Using a signal for the form draft mapped strictly to DB schema
    let mut draft_theme = use_signal(|| "system".to_string());
    let mut draft_provider = use_signal(|| "openai".to_string());
    let mut draft_llm_model = use_signal(|| "gpt-5.4-mini".to_string());
    let mut draft_transcript = use_signal(|| "medium".to_string());

    let mut is_loaded = use_signal(|| false);
    let mut action_error = use_signal(|| None::<String>);

    let mut is_modal_open = use_signal(|| false);
    let mut theme_sig = use_context::<crate::components::nav::ThemeState>().0;
    let mut modal_input = use_signal(String::new);
    let masked_key = use_signal(String::new);
    let mut modal_error = use_signal(|| None::<String>);

    let update_masked_key = |mut mk: Signal<String>| match read_api_key() {
        Ok(key) if !key.is_empty() => {
            if key.len() > 4 {
                let first = &key[0..2];
                let last = &key[key.len() - 2..];
                mk.set(format!("{}...{}", first, last));
            } else {
                mk.set("...".to_string());
            }
        }
        _ => mk.set("No Api Key Configured".to_string()),
    };

    use_effect(move || {
        update_masked_key(masked_key);
    });

    let state_for_load = state.clone();
    let mut data = use_resource(move || {
        let model = SettingsPageModel::new(state_for_load.clone());
        async move { model.load().await.map_err(|e| format!("{e:#}")) }
    });

    // Populate draft when data loads
    use_effect(move || {
        if !is_loaded()
            && let Some(Ok(d)) = &*data.read()
        {
            draft_theme.set(d.settings.ui_theme.clone());
            draft_provider.set(d.settings.llm_provider.clone());
            draft_llm_model.set(d.settings.llm_model.clone());
            draft_transcript.set(d.settings.transcript_model.clone());

            is_loaded.set(true);
        }
    });

    let state_for_save = state.clone();
    let on_save = move |_| {
        let model = SettingsPageModel::new(state_for_save.clone());
        let theme = draft_theme();
        let provider = draft_provider();
        let llm_model = draft_llm_model();
        let transcript = draft_transcript();

        spawn(async move {
            let mut updated_settings = None;
            if let Some(Ok(d)) = &*data.read() {
                let mut s = d.settings.clone();
                s.ui_theme = theme;
                s.llm_provider = provider;
                s.llm_model = llm_model;
                s.transcript_model = transcript;

                updated_settings = Some(s);
            }
            if let Some(s) = updated_settings {
                let theme_to_set = s.ui_theme.clone();
                if let Err(e) = model.update(s).await {
                    tracing::error!("Failed to save settings: {e:#}");
                    action_error.set(Some(e.to_string()));
                    return;
                } else {
                    action_error.set(None);
                    theme_sig.set(theme_to_set);
                }
                data.restart();
            }
        });
    };

    let on_reset = move |_| {
        if let Some(Ok(d)) = &*data.read() {
            draft_theme.set(d.settings.ui_theme.clone());
            draft_provider.set(d.settings.llm_provider.clone());
            draft_llm_model.set(d.settings.llm_model.clone());
            draft_transcript.set(d.settings.transcript_model.clone());
        }
    };

    let has_changes = if let Some(Ok(d)) = &*data.read() {
        draft_theme() != d.settings.ui_theme
            || draft_provider() != d.settings.llm_provider
            || draft_llm_model() != d.settings.llm_model
            || draft_transcript() != d.settings.transcript_model
    } else {
        false
    };

    rsx! {
        div { class: "flex h-full flex-col overflow-hidden rounded-lg border border-hairline bg-canvas shadow-sm",
            div { class: "shrink-0 flex items-center justify-between gap-4 border-b border-hairline bg-surface-soft px-4 py-3",
                div {
                    h2 { class: "text-lg font-semibold text-ink", "Settings" }
                    p { class: "text-sm text-body",
                        "Edit runtime defaults. Provider fields are stored locally in SQLite."
                    }
                }
                div { class: "flex gap-2",
                    button {
                        class: "rounded-md px-4 py-2 text-sm font-semibold text-body hover:bg-surface-strong",
                        disabled: !has_changes,
                        onclick: on_reset,
                        "Reset"
                    }
                    button {
                        class: if has_changes { "rounded-md bg-primary px-4 py-2 text-sm font-semibold text-on-primary hover:bg-primary-active" } else { "cursor-not-allowed rounded-md bg-primary-disabled px-4 py-2 text-sm font-semibold text-on-primary" },
                        disabled: !has_changes,
                        onclick: on_save,
                        "Save"
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

            div { class: "min-h-0 flex-1 divide-y divide-hairline px-4 pb-12 overflow-y-auto",
                if is_loaded() {
                    div { class: "py-6",
                        h3 { class: "mb-4 text-sm font-bold uppercase tracking-[0.08em] text-muted",
                            "Appearance"
                        }
                        div { class: "grid gap-6",
                            div { class: "flex items-center justify-between",
                                div {
                                    label { class: "block font-semibold text-ink", "Dark Mode" }
                                    p { class: "text-sm text-body", "Toggle dark appearance." }
                                }
                                button {
                                    class: if draft_theme() == "dark" { "relative inline-flex h-6 w-11 shrink-0 cursor-pointer items-center rounded-full bg-primary transition-colors focus:outline-none" } else { "relative inline-flex h-6 w-11 shrink-0 cursor-pointer items-center rounded-full bg-surface-strong transition-colors focus:outline-none" },
                                    onclick: move |_| {
                                        if draft_theme() == "dark" {
                                            draft_theme.set("light".to_string());
                                        } else {
                                            draft_theme.set("dark".to_string());
                                        }
                                    },
                                    span { class: if draft_theme() == "dark" { "inline-block h-4 w-4 translate-x-6 transform rounded-full bg-white transition-transform" } else { "inline-block h-4 w-4 translate-x-1 transform rounded-full bg-white transition-transform" } }
                                }
                            }
                        }
                    }

                    div { class: "py-6",
                        h3 { class: "mb-4 text-sm font-bold uppercase tracking-[0.08em] text-muted",
                            "Transcription"
                        }
                        div { class: "grid gap-6",
                            div { class: "flex items-center justify-between",
                                div {
                                    label { class: "block font-semibold text-ink", "Moonshine Model" }
                                    p { class: "text-sm text-body",
                                        "The size of the Moonshine STT model."
                                    }
                                }
                                select {
                                    class: "rounded-lg border border-hairline bg-surface-soft px-3 py-2 text-sm text-ink outline-none focus:border-primary",
                                    value: "{draft_transcript}",
                                    onchange: move |e| draft_transcript.set(e.value()),
                                    option { value: "tiny", "Tiny" }
                                    option { value: "small", "Small" }
                                    option { value: "medium", "Medium" }
                                }
                            }
                        }
                    }

                    div { class: "py-6",
                        h3 { class: "mb-4 text-sm font-bold uppercase tracking-[0.08em] text-muted",
                            "Assistant"
                        }
                        div { class: "grid gap-6",
                            div { class: "flex items-center justify-between",
                                div {
                                    label { class: "block font-semibold text-ink", "LLM Provider" }
                                    p { class: "text-sm text-body",
                                        "The backend service for generation."
                                    }
                                }
                                select {
                                    class: "cursor-not-allowed opacity-70 rounded-lg border border-hairline bg-surface-soft px-3 py-2 text-sm text-ink outline-none focus:border-primary",
                                    value: "{draft_provider}",
                                    disabled: true,
                                    onchange: move |e| draft_provider.set(e.value()),
                                    option { value: "openai", "OpenAI" }
                                }
                            }
                            div { class: "flex items-center justify-between",
                                div {
                                    label { class: "block font-semibold text-ink", "API Key" }
                                    p { class: "text-sm text-body",
                                        "Securely stored in your OS keychain."
                                    }
                                }
                                button {
                                    class: "rounded-md bg-surface-strong px-4 py-2 text-sm font-semibold text-ink hover:bg-surface-soft border border-hairline",
                                    onclick: move |_| {
                                        modal_input.set(String::new());
                                        modal_error.set(None);
                                        is_modal_open.set(true);
                                    },
                                    "Manage"
                                }
                            }
                            div { class: "flex items-center justify-between",
                                div {
                                    label { class: "block font-semibold text-ink", "LLM Model" }
                                    p { class: "text-sm text-body", "The generative model to use." }
                                }
                                input {
                                    class: "w-64 rounded-lg border border-hairline bg-surface-soft px-3 py-2 text-sm text-ink outline-none focus:border-primary",
                                    value: "{draft_llm_model}",
                                    oninput: move |e| draft_llm_model.set(e.value().clone()),
                                }
                            }
                        }
                    }
                } else if let Some(Err(msg)) = &*data.read() {
                    ErrorView {
                        message: msg.clone(),
                        on_retry: move |_| data.restart(),
                    }
                } else {
                    div { class: "p-6 text-center text-sm text-body", "Loading settings..." }
                }
            }
        }

        if is_modal_open() {
            div { class: "fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-sm",
                div { class: "w-full max-w-md rounded-lg bg-canvas p-6 shadow-xl border border-hairline",
                    h2 { class: "text-lg font-bold text-ink mb-2", "Manage API Key" }
                    p { class: "text-sm text-body mb-4",
                        "Your API key will be securely stored in your native OS keychain. You can overwrite it or remove it."
                    }

                    if let Some(err) = modal_error() {
                        div { class: "mb-4 text-sm text-red-500", "{err}" }
                    }

                    input {
                        class: "w-full rounded-md border border-hairline bg-surface-soft px-3 py-2 text-sm text-ink outline-none focus:border-primary mb-6",
                        placeholder: "{masked_key}",
                        value: "{modal_input}",
                        oninput: move |e| modal_input.set(e.value().clone()),
                        r#type: "text",
                    }

                    div { class: "flex justify-end gap-3",
                        if masked_key() != "No Api Key Configured" {
                            button {
                                class: "rounded-md px-4 py-2 text-sm font-semibold text-body hover:bg-surface-strong",
                                onclick: move |_| {
                                    if let Err(e) = delete_api_key() {
                                        modal_error.set(Some(e.to_string()));
                                    } else {
                                        update_masked_key(masked_key);
                                        is_modal_open.set(false);
                                    }
                                },
                                "Remove Key"
                            }
                        }
                        button {
                            class: "rounded-md px-4 py-2 text-sm font-semibold text-body hover:bg-surface-strong",
                            onclick: move |_| is_modal_open.set(false),
                            "Cancel"
                        }
                        button {
                            class: if modal_input().is_empty() { "cursor-not-allowed rounded-md bg-primary-disabled px-4 py-2 text-sm font-semibold text-on-primary" } else { "rounded-md bg-primary px-4 py-2 text-sm font-semibold text-on-primary hover:bg-primary-active" },
                            disabled: modal_input().is_empty(),
                            onclick: move |_| {
                                let key = modal_input();
                                if key.is_empty() {
                                    modal_error.set(Some("Please enter a key".to_string()));
                                    return;
                                }
                                if let Err(e) = store_api_key(&key) {
                                    modal_error.set(Some(e.to_string()));
                                } else {
                                    update_masked_key(masked_key);
                                    is_modal_open.set(false);
                                }
                            },
                            "Save"
                        }
                    }
                }
            }
        }
    }
}
