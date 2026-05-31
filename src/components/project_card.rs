use dioxus::prelude::*;

#[derive(Clone, PartialEq)]
pub struct ProjectCardData {
    pub id: String,
    pub name: String,
    pub description: String,
}

#[component]
pub fn EmptyProjectCard() -> Element {
    rsx! {
        div { class: "rounded-md border border-dashed border-hairline p-8 text-center text-body w-full",
            "No projects yet. Create one to begin."
        }
    }
}

#[component]
pub fn ProjectCardView(
    project: ProjectCardData,
    onclick: EventHandler<MouseEvent>,
    onedit: EventHandler<MouseEvent>,
    ondelete: EventHandler<MouseEvent>,
) -> Element {
    rsx! {
        div { class: "rounded-md hover:bg-surface-soft border-b border-hairline last:border-none flex items-center justify-between group",
            button {
                class: "flex-1 px-3 py-3 text-left w-full",
                onclick: move |event| onclick.call(event),
                div {
                    p { class: "font-semibold text-ink", "{project.name}" }
                    p { class: "truncate text-sm text-body",
                        if project.description.is_empty() {
                            "No description"
                        } else {
                            "{project.description}"
                        }
                    }
                }
            }
            div { class: "px-3 opacity-0 group-hover:opacity-100 flex gap-2 transition-opacity",
                button {
                    class: "rounded text-sm font-semibold text-body hover:bg-surface-strong px-3 py-1.5",
                    onclick: move |e| { e.stop_propagation(); onedit.call(e) },
                    "Edit"
                }
                button {
                    class: "rounded text-sm font-semibold text-semantic-down hover:bg-semantic-down/10 px-3 py-1.5",
                    onclick: move |e| { e.stop_propagation(); ondelete.call(e) },
                    "Delete"
                }
            }
        }
    }
}
