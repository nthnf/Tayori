pub mod backend;
pub mod components;
pub mod pages;
pub mod state;

use crate::components::nav::RootLayout;
use crate::pages::{
    dashboard::DashboardPage, live::LivePage, project::ProjectPage, settings::SettingsPage,
};
use dioxus::prelude::*;

#[derive(Routable, Clone, Debug, PartialEq)]
pub enum Route {
    #[layout(RootLayout)]
    #[route("/")]
    DashboardPage,

    #[route("/settings")]
    SettingsPage,

    #[route("/project/:id")]
    ProjectPage { id: String },

    #[route("/live/:project_id/:session_id")]
    LivePage {
        project_id: String,
        session_id: String,
    },
}
