use dioxus::prelude::*;

use crate::types::Page;

#[component]
pub fn TopNav(mut page: Signal<Page>, title: String) -> Element {
    let action_label = if page() == Page::Settings {
        "Home"
    } else {
        "Settings"
    };
    let action_page = if page() == Page::Settings {
        Page::Dashboard
    } else {
        Page::Settings
    };
    rsx! {
        header { class: "grid h-12 grid-cols-[1fr_auto_1fr] items-center border-b border-hairline bg-surface-soft px-4 text-ink",
            button { class: "justify-self-start text-sm font-bold tracking-[-0.02em] text-primary", onclick: move |_| page.set(Page::Dashboard), "Tayori" }
            h1 { class: "justify-self-center text-sm font-semibold", "{title}" }
            button { class: "justify-self-end rounded-md px-3 py-1.5 text-sm font-semibold text-body hover:bg-surface-strong hover:text-ink", onclick: move |_| page.set(action_page), "{action_label}" }
        }
    }
}

#[component]
pub fn NavButton(active: bool, label: String, onclick: EventHandler<MouseEvent>) -> Element {
    rsx! {
        button {
            class: if active { "flex min-h-11 w-full items-center justify-start rounded-full bg-surface-strong px-5 py-3 text-base font-semibold text-ink" } else { "flex min-h-11 w-full items-center justify-start rounded-full px-5 py-3 text-base font-semibold text-body" },
            onclick: move |event| onclick.call(event),
            "{label}"
        }
    }
}
