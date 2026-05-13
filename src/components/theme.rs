use dioxus::prelude::*;

#[component]
pub fn ThemeButton(selected: bool, label: String, onclick: EventHandler<MouseEvent>) -> Element {
    rsx! {
        button {
            class: if selected { "min-h-10 rounded-full bg-primary px-4 py-2.5 text-base font-semibold text-on-primary" } else { "min-h-10 rounded-full bg-surface-strong px-4 py-2.5 text-base font-semibold text-body" },
            onclick: move |event| onclick.call(event),
            "{label}"
        }
    }
}
