use crate::Route;
use dioxus::prelude::*;

#[derive(Clone, Copy)]
pub struct ThemeState(pub Signal<String>);

#[component]
pub fn RootLayout() -> Element {
    let route = use_route::<Route>();
    let navigator = use_navigator();
    let state = use_context::<crate::state::AppState>();

    let mut theme_sig = use_signal(|| "light".to_string());
    use_context_provider(|| ThemeState(theme_sig));

    let _theme = use_resource(move || {
        let state = state.clone();
        async move {
            let model = crate::backend::pages::settings::SettingsPageModel::new(state);
            let t = model
                .load()
                .await
                .map(|d| d.settings.ui_theme)
                .unwrap_or_else(|_| "light".to_string());
            theme_sig.set(t.clone());
            t
        }
    });

    let theme_val = theme_sig();
    let theme_class = match theme_val.as_str() {
        "dark" => "dark",
        _ => "",
    };

    let (title, can_go_back, back_route, action_label, action_route) = match route {
        Route::DashboardPage => (
            "Projects".to_string(),
            false,
            None,
            "Settings".to_string(),
            Route::SettingsPage,
        ),
        Route::ProjectPage { id: _ } => (
            "Project".to_string(),
            true,
            Some(Route::DashboardPage),
            "Settings".to_string(),
            Route::SettingsPage,
        ),
        Route::LivePage {
            project_id,
            session_id: _,
        } => (
            "Live Recording".to_string(),
            true,
            Some(Route::ProjectPage {
                id: project_id.clone(),
            }),
            "Settings".to_string(),
            Route::SettingsPage,
        ),
        Route::SettingsPage => (
            "Settings".to_string(),
            false,
            None,
            "Home".to_string(),
            Route::DashboardPage,
        ),
    };

    rsx! {
        main { class: "{theme_class} min-h-screen bg-canvas text-ink font-sans transition-colors duration-200",
            div { class: "flex h-screen w-full flex-col overflow-hidden",
                header { class: "grid h-12 grid-cols-[1fr_auto_1fr] items-center border-b border-hairline bg-surface-soft px-4 text-ink",
                    div { class: "flex items-center gap-2 justify-self-start",
                        if can_go_back {
                            button {
                                class: "grid h-8 w-8 place-items-center rounded-md text-lg font-semibold text-body hover:bg-surface-strong hover:text-ink",
                                onclick: move |_| {
                                    if let Some(route) = back_route.clone() {
                                        navigator.push(route);
                                    } else {
                                        navigator.go_back();
                                    }
                                },
                                "‹"
                            }
                        }
                        button {
                            class: "text-sm font-bold tracking-[-0.02em] text-primary",
                            onclick: move |_| {
                                navigator.push(Route::DashboardPage);
                            },
                            "Tayori"
                        }
                    }
                    h1 { class: "justify-self-center text-sm font-semibold", "{title}" }
                    button {
                        class: "justify-self-end rounded-md px-3 py-1.5 text-sm font-semibold text-body hover:bg-surface-strong hover:text-ink",
                        onclick: move |_| {
                            navigator.push(action_route.clone());
                        },
                        "{action_label}"
                    }
                }
                section { class: "min-h-0 flex-1 overflow-hidden bg-surface-soft p-4", Outlet::<Route> {} }
            }
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
