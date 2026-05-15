use dioxus::prelude::*;
use tayori_core::service::{
    SettingsFormView, WhisperModelView, DENSE_MODEL_OPTIONS, RERANKER_MODEL_OPTIONS,
    SPARSE_MODEL_OPTIONS,
};

use crate::types::Theme;

#[component]
pub fn SettingsPage(
    mut theme: Signal<Theme>,
    mut settings: Signal<SettingsFormView>,
    saved_settings: Signal<SettingsFormView>,
    whisper_models: Signal<Vec<WhisperModelView>>,
    on_check_whisper_models: EventHandler<MouseEvent>,
    on_install_whisper_model: EventHandler<String>,
    on_remove_whisper_model: EventHandler<String>,
    on_save: EventHandler<SettingsFormView>,
) -> Element {
    let mut show_api_key_dialog = use_signal(|| false);
    let mut show_whisper_dialog = use_signal(|| false);
    let mut show_rebuild_dialog = use_signal(|| false);
    let has_changes = settings() != saved_settings();
    let search_models_changed = {
        let settings = settings.read();
        let saved = saved_settings.read();
        settings.embedding_model != saved.embedding_model
            || settings.sparse_model != saved.sparse_model
    };

    let save_changes = move |_| {
        if search_models_changed {
            show_rebuild_dialog.set(true);
        } else {
            on_save.call(settings());
        }
    };

    let reset_changes = move |_| {
        let saved = saved_settings();
        theme.set(theme_from_settings(&saved.ui_theme));
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
                        settings.write().ui_theme = theme_setting(next);
                    }
                }
                ReadOnlyRow { name: "LLM provider".to_string(), description: "Provider used for suggested answers and summaries.".to_string(), value: settings.read().llm_provider.clone() }
                TextRow { name: "LLM model".to_string(), description: "Model name used by the selected LLM provider.".to_string(), value: settings.read().llm_model.clone(), placeholder: "gpt-4.1-mini".to_string(), oninput: move |value| settings.write().llm_model = value }
                ApiKeyRow {
                    preview: settings.read().llm_api_key_preview.clone(),
                    marked_for_removal: settings.read().remove_llm_api_key,
                    on_manage: move |_| show_api_key_dialog.set(true),
                }
                ReadOnlyRow { name: "Embedding provider".to_string(), description: "Embedding runtime used for document chunks and transcript summaries.".to_string(), value: settings.read().embedding_provider.clone() }
                SelectRow { name: "Dense model".to_string(), description: "Model used to understand document and meeting text.".to_string(), value: settings.read().embedding_model.clone(), options: model_options(DENSE_MODEL_OPTIONS), oninput: move |value| settings.write().embedding_model = value }
                SelectRow { name: "Sparse model".to_string(), description: "Model used to match exact words and phrases in saved content.".to_string(), value: settings.read().sparse_model.clone(), options: model_options(SPARSE_MODEL_OPTIONS), oninput: move |value| settings.write().sparse_model = value }
                SelectRow { name: "Rerank model".to_string(), description: "Model used to sort the best matches before answering.".to_string(), value: settings.read().reranker_model.clone(), options: model_options(RERANKER_MODEL_OPTIONS), oninput: move |value| settings.write().reranker_model = value }
                WhisperModelRow {
                    model_name: settings.read().whisper_model.clone(),
                    oninput: move |value| settings.write().whisper_model = value,
                    on_manage: move |event| {
                        on_check_whisper_models.call(event);
                        show_whisper_dialog.set(true);
                    }
                }
                TextRow { name: "Summary window".to_string(), description: "Minutes of transcript chunks to combine into each summary. Valid range is 1 to 10.".to_string(), value: settings.read().summary_minutes.clone(), placeholder: "5".to_string(), oninput: move |value| settings.write().summary_minutes = value }
            }

            if show_api_key_dialog() {
                ApiKeyDialog {
                    preview: settings.read().llm_api_key_preview.clone(),
                    new_value: settings.read().new_llm_api_key.clone(),
                    oninput: move |value| {
                        settings.write().new_llm_api_key = value;
                        settings.write().remove_llm_api_key = false;
                    },
                    on_remove: move |_| {
                        settings.write().new_llm_api_key.clear();
                        settings.write().llm_api_key_preview.clear();
                        settings.write().remove_llm_api_key = true;
                        show_api_key_dialog.set(false);
                    },
                    on_accept: move |api_key| {
                        let mut form = settings();
                        form.new_llm_api_key = api_key;
                        form.remove_llm_api_key = false;
                        settings.set(form.clone());
                        show_api_key_dialog.set(false);
                        if search_models_changed {
                            show_rebuild_dialog.set(true);
                        } else {
                            on_save.call(form);
                        }
                    },
                    on_close: move |_| show_api_key_dialog.set(false),
                }
            }

            if show_whisper_dialog() {
                WhisperModelDialog {
                    models: whisper_models(),
                    selected_model: settings.read().whisper_model.clone(),
                    on_install: move |model_name| on_install_whisper_model.call(model_name),
                    on_remove: move |model_name| on_remove_whisper_model.call(model_name),
                    on_close: move |_| show_whisper_dialog.set(false),
                }
            }

            if show_rebuild_dialog() {
                RebuildWarningDialog {
                    on_cancel: move |_| show_rebuild_dialog.set(false),
                    on_confirm: move |_| {
                        show_rebuild_dialog.set(false);
                        on_save.call(settings());
                    },
                }
            }
        }
    }
}

fn model_options(options: &[&str]) -> Vec<String> {
    options.iter().map(|option| option.to_string()).collect()
}

#[component]
fn WhisperModelRow(
    model_name: String,
    oninput: EventHandler<String>,
    on_manage: EventHandler<MouseEvent>,
) -> Element {
    rsx! {
        div { class: "grid gap-3 py-4 md:grid-cols-[minmax(220px,1fr)_minmax(280px,420px)] md:items-center",
            div {
                p { class: "font-semibold text-ink", "Whisper model" }
                p { class: "mt-1 text-sm text-body", "Local speech-to-text model name." }
            }
            div { class: "flex gap-2",
                input { class: "min-w-0 flex-1 rounded-md border border-hairline bg-canvas px-3 py-2 text-ink outline-none focus:border-primary", value: model_name, placeholder: "small-q8_0", oninput: move |event| oninput.call(event.value()) }
                button { class: "rounded-md bg-surface-strong px-3 py-2 text-sm font-semibold text-ink hover:bg-hairline", onclick: move |event| on_manage.call(event), "Manage" }
            }
        }
    }
}

#[component]
fn WhisperModelDialog(
    models: Vec<WhisperModelView>,
    selected_model: String,
    on_install: EventHandler<String>,
    on_remove: EventHandler<String>,
    on_close: EventHandler<MouseEvent>,
) -> Element {
    rsx! {
        div { class: "absolute inset-0 z-20 grid place-items-center bg-black/30 p-4",
            div { class: "flex max-h-[82vh] w-full max-w-2xl flex-col rounded-lg border border-hairline bg-canvas p-5 shadow-2xl",
                div { class: "mb-4 flex items-start justify-between gap-4",
                    div {
                        h3 { class: "text-lg font-semibold text-ink", "Manage Whisper model" }
                        p { class: "mt-1 text-sm text-body", "Install or remove local speech-to-text models." }
                    }
                    button { class: "rounded-md px-2 py-1 text-body hover:bg-surface-soft", onclick: move |event| on_close.call(event), "Close" }
                }

                div { class: "min-h-0 overflow-auto rounded-md border border-hairline",
                    div { class: "grid grid-cols-[1fr_120px_100px] gap-3 border-b border-hairline bg-surface-soft px-4 py-2 text-xs font-semibold uppercase tracking-[0.08em] text-muted",
                        span { "Model" }
                        span { "Status" }
                        span { "Action" }
                    }
                    div { class: "divide-y divide-hairline",
                        for model in models {
                            WhisperModelListRow {
                                model: model.clone(),
                                selected: model.model_name == selected_model,
                                on_install,
                                on_remove,
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn WhisperModelListRow(
    model: WhisperModelView,
    selected: bool,
    on_install: EventHandler<String>,
    on_remove: EventHandler<String>,
) -> Element {
    rsx! {
        div { class: "grid grid-cols-[1fr_120px_100px] gap-3 px-4 py-3 text-sm hover:bg-surface-soft",
            div {
                p { class: "font-semibold text-ink", "{model.model_name}" }
                if selected {
                    p { class: "mt-1 text-xs text-primary", "Selected in settings" }
                }
            }
            span { class: if model.installed { "self-center text-semantic-up" } else { "self-center text-body" }, if model.installed { "Installed" } else { "Not installed" } }
            if model.installed {
                button { class: "self-center rounded-md bg-semantic-down/10 px-3 py-1.5 text-xs font-semibold text-semantic-down hover:bg-semantic-down/20", onclick: move |_| on_remove.call(model.model_name.clone()), "Remove" }
            } else {
                button { class: "self-center rounded-md bg-primary px-3 py-1.5 text-xs font-semibold text-on-primary hover:bg-primary-active", onclick: move |_| on_install.call(model.model_name.clone()), "Install" }
            }
        }
    }
}

fn theme_setting(theme: Theme) -> String {
    match theme {
        Theme::Light => "light".to_string(),
        Theme::Dark => "dark".to_string(),
    }
}

fn theme_from_settings(value: &str) -> Theme {
    if value == "dark" {
        Theme::Dark
    } else {
        Theme::Light
    }
}

#[component]
fn ApiKeyRow(
    preview: String,
    marked_for_removal: bool,
    on_manage: EventHandler<MouseEvent>,
) -> Element {
    let current = if marked_for_removal {
        "Marked for removal on save".to_string()
    } else if preview.is_empty() {
        "No API key configured".to_string()
    } else {
        preview
    };

    rsx! {
        div { class: "grid gap-3 py-4 md:grid-cols-[minmax(220px,1fr)_minmax(280px,420px)] md:items-center",
            div {
                p { class: "font-semibold text-ink", "API key" }
                p { class: "mt-1 text-sm text-body", "Stored in the OS keyring. Add a new key to replace the existing one, or remove it." }
            }
            div { class: "grid grid-cols-[minmax(0,1fr)_auto] items-center gap-2",
                div { class: "min-w-0 truncate rounded-md border border-hairline bg-surface-soft px-3 py-2 text-body", "{current}" }
                button { class: "rounded-md bg-surface-strong px-3 py-2 text-sm font-semibold text-ink hover:bg-hairline", onclick: move |event| on_manage.call(event), "Manage" }
            }
        }
    }
}

#[component]
fn ApiKeyDialog(
    preview: String,
    new_value: String,
    oninput: EventHandler<String>,
    on_remove: EventHandler<MouseEvent>,
    on_accept: EventHandler<String>,
    on_close: EventHandler<MouseEvent>,
) -> Element {
    let has_key = !preview.is_empty();
    let title = if has_key {
        "Replace API key"
    } else {
        "Create API key"
    };
    let action = if has_key { "Replace" } else { "Create" };
    let accept_value = new_value.clone();

    rsx! {
        div { class: "absolute inset-0 z-20 grid place-items-center bg-black/30 p-4",
            div { class: "w-full max-w-md rounded-lg border border-hairline bg-canvas p-5 shadow-2xl",
                div { class: "mb-4 flex items-start justify-between gap-4",
                    div {
                        h3 { class: "text-lg font-semibold text-ink", "{title}" }
                        p { class: "mt-1 text-sm text-body", "Only one LLM API key is stored. Saving a new key fully replaces the previous key in the OS keyring." }
                    }
                    button { class: "rounded-md px-2 py-1 text-body hover:bg-surface-soft", onclick: move |event| on_close.call(event), "Close" }
                }

                if has_key {
                    div { class: "mb-4 rounded-md border border-hairline bg-surface-soft px-3 py-2 text-sm text-body", "Current: {preview}" }
                }

                input { class: "w-full rounded-md border border-hairline bg-canvas px-3 py-2 text-ink outline-none focus:border-primary", r#type: "text", value: new_value, placeholder: "New API key", oninput: move |event| oninput.call(event.value()) }

                div { class: "mt-5 flex justify-between gap-2",
                    div {
                        if has_key {
                            button { class: "rounded-md bg-semantic-down/10 px-4 py-2 text-sm font-semibold text-semantic-down hover:bg-semantic-down/20", onclick: move |event| on_remove.call(event), "Remove" }
                        }
                    }
                    div { class: "flex gap-2",
                        button { class: "rounded-md px-4 py-2 text-sm font-semibold text-body hover:bg-surface-soft", onclick: move |event| on_close.call(event), "Cancel" }
                        button { class: "rounded-md bg-primary px-4 py-2 text-sm font-semibold text-on-primary hover:bg-primary-active", onclick: move |_| on_accept.call(accept_value.clone()), "{action}" }
                    }
                }
            }
        }
    }
}

#[component]
fn RebuildWarningDialog(
    on_cancel: EventHandler<MouseEvent>,
    on_confirm: EventHandler<MouseEvent>,
) -> Element {
    rsx! {
        div { class: "absolute inset-0 z-20 grid place-items-center bg-black/30 p-4",
            div { class: "w-full max-w-md rounded-lg border border-hairline bg-canvas p-5 shadow-2xl",
                h3 { class: "text-lg font-semibold text-ink", "Prepare saved content again?" }
                p { class: "mt-2 text-sm leading-relaxed text-body", "Changing the dense or sparse model means Tayori needs to prepare your existing documents and meeting notes again so search and answers keep working. This may take some time." }
                div { class: "mt-5 flex justify-end gap-2",
                    button { class: "rounded-md px-4 py-2 text-sm font-semibold text-body hover:bg-surface-soft", onclick: move |event| on_cancel.call(event), "Cancel" }
                    button { class: "rounded-md bg-primary px-4 py-2 text-sm font-semibold text-on-primary hover:bg-primary-active", onclick: move |event| on_confirm.call(event), "Continue" }
                }
            }
        }
    }
}

#[component]
fn SelectRow(
    name: String,
    description: String,
    value: String,
    options: Vec<String>,
    oninput: EventHandler<String>,
) -> Element {
    rsx! {
        div { class: "grid gap-3 py-4 md:grid-cols-[minmax(220px,1fr)_minmax(280px,420px)] md:items-center",
            div {
                p { class: "font-semibold text-ink", "{name}" }
                p { class: "mt-1 text-sm text-body", "{description}" }
            }
            select { class: "rounded-md border border-hairline bg-canvas px-3 py-2 text-black outline-none focus:border-primary", value, onchange: move |event| oninput.call(event.value()),
                for option in options {
                    option { class: "text-black", value: option.clone(), "{option}" }
                }
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
