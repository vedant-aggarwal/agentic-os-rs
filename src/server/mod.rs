//! Server bootstrap for the fullstack build (`--features server`).
//!
//! Instead of `dioxus::launch`, the server build runs its own Tokio `main` (research/tech-stack.md
//! R6): the Agentic OS API router from `aos-server` with the Dioxus SSR router merged in, served
//! by `axum::serve` with graceful shutdown. Listens on `AOS_ADDR`, or `IP`/`PORT` as `dx serve`
//! sets them. Trade-off: server-side hot-patching of the router (which `dioxus::serve` wires up)
//! is not available here; client hot-reload is unaffected.

use dioxus::prelude::Element;

pub fn launch(app: fn() -> Element) {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to start the Tokio runtime");
    let result = runtime.block_on(async move {
        aos_server::init_tracing();
        let config = aos_server::Config::from_env()?;
        ensure_public_dir();
        // Work tab: the session supervisor shares the database file; its tools join the registry.
        let work = aos_work::Work::start(aos_work::WorkConfig { db_path: config.db_path.clone(), ..aos_work::WorkConfig::from_env() })?;
        work.install_global();
        let mut registry = aos_work::tools::registry(&work);
        // Nora: her Claude turns reach our tools through this server's MCP endpoint.
        let mut nora_cfg = aos_nora::NoraConfig::from_env(config.db_path.clone());
        nora_cfg.mcp_url = Some(mcp_url(config.addr));
        let nora = aos_nora::Nora::start(nora_cfg)?;
        nora.install_global();
        registry.extend(aos_nora::tools::tools(&nora));
        // Morning print: `print_morning`, `morning_settings`, `plan_photo`; the 05:00 job runs in Nora's scheduler.
        registry.extend(aos_nora::morning::tools(&nora));
        // Voice calls: Codex realtime for the voice, hand-offs as Claude turns.
        let calls = aos_nora::call::Calls::start(nora.clone(), aos_nora::call::CallConfig::from_env())?;
        registry.extend(aos_nora::call::tools(&nora));
        // Inbox: WhatsApp (wacli), Gmail (gws) and Laya triage run as supervised workers.
        let inbox = aos_inbox::Inbox::start(&config.db_path, aos_inbox::InboxConfig::load())?;
        registry.extend(aos_inbox::tools::tools_with(&inbox));
        let auth_store = std::sync::Arc::new(aos_work::AuthStore::open(&config.db_path)?);
        let work_routes = aos_work::http::router_with_auth(work.clone(), auth_store.clone());
        let nora_routes = aos_nora::http::router_with_auth(nora.clone(), auth_store.clone());
        let call_routes = aos_nora::call::http::router_with_auth(calls, auth_store.clone());
        // Phone notifications: alarms, rings, approvals and important inbox items reach the phone.
        let push = aos_push::Push::start(&config.db_path)?;
        push.watch(nora.bus().subscribe(), work.subscribe(), Some(inbox.subscribe_important()));
        let push_routes = aos_push::http::router(push, auth_store.clone());
        // AOS_WEB_DIR set (the React build in web/dist): serve that at `/` instead of the Dioxus UI.
        let web_dir = aos_server::web::web_dir_from_env();
        // Dioxus server functions live under /api/ and need a device token, like /api/work.
        let ui = if web_dir.is_none() { Some(aos_server::auth::require_device_under(dioxus::server::router(app), auth_store, "/api/")) } else { None };
        // API routes first; the Dioxus router (or the static web UI) brings assets and the page fallback.
        let served = aos_server::serve_with(config, registry, move |api| {
            let api = api.merge(work_routes).merge(nora_routes).merge(call_routes).merge(push_routes);
            match (ui, web_dir) {
                (Some(ui), _) => api.merge(ui),
                (None, Some(dir)) => aos_server::web::with_web(api, &dir),
                (None, None) => api,
            }
        })
        .await;
        nora.stop();
        work.stop_all();
        inbox.stop();
        inbox.wait_stopped(std::time::Duration::from_secs(6)).await;
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        served
    });
    if let Err(e) = result {
        eprintln!("server error: {e:#}");
        std::process::exit(1);
    }
}

/// The MCP endpoint of this server as a local client reaches it.
fn mcp_url(addr: std::net::SocketAddr) -> String {
    let host = if addr.ip().is_unspecified() { "127.0.0.1".to_string() } else { addr.ip().to_string() };
    format!("http://{host}:{}/mcp", addr.port())
}

/// `dioxus::server::router` panics when `<exe dir>/public` is missing. `dx build` / `dx serve`
/// create it with the web bundle; a plain `cargo build` doesn't. Create an empty one so the API
/// and SSR still run (pages render without the wasm client until `dx build` fills it).
fn ensure_public_dir() {
    let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join("public")))
    else {
        return;
    };
    if !dir.exists() {
        eprintln!("warning: {} missing; creating it empty. Run `dx build --platform web` for the web client.", dir.display());
        let _ = std::fs::create_dir_all(&dir);
    }
}
