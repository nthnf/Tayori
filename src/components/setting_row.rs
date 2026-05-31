use dioxus::prelude::*;

#[component]
pub fn SettingRow(label: String, value: String) -> Element {
    rsx! {
        div { class: "flex items-center justify-between rounded-xl bg-surface-soft px-4 py-3",
            span { class: "text-body", "{label}" }
            span { class: "font-mono text-sm font-medium text-ink", "{value}" }
        }
    }
}
