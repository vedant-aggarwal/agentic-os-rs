mod app;
mod components;
#[allow(dead_code)]
mod data;
mod live;
mod pages;
mod state;
mod theme;

#[cfg(feature = "server")]
mod server;

fn main() {
    // The server build runs its own Axum main (API + SSR, graceful shutdown): see src/server/.
    #[cfg(feature = "server")]
    server::launch(app::App);
    #[cfg(not(feature = "server"))]
    dioxus::launch(app::App);
}
