mod app;
mod components;
mod data;
mod pages;
mod state;
mod theme;

fn main() {
    dioxus::launch(app::App);
}
