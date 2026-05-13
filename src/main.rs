mod app;
mod components;
mod pages;
mod types;

use app::App;

fn main() {
    dioxus::launch(App);
}
