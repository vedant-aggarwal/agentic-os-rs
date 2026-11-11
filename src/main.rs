mod app;
mod call;
mod components;
#[allow(dead_code)]
mod data;
mod live;
mod native;
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
    {
        // Push hooks go in before UIApplicationMain, so a tap that launched the app is not lost.
        native::Native::install();
        dioxus::launch(app::App);
    }
}
