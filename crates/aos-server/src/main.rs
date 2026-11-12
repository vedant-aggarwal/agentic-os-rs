//! `aos-server`: the Agentic OS API on its own (no Nora, Work or inbox).
//!
//! ```text
//! aos-server [--db PATH] [--addr HOST:PORT] [--web DIR]
//! ```
//! Defaults: `%USERPROFILE%\.aos\aos.db` (or `AOS_DB`) and `127.0.0.1:8787` (or `AOS_ADDR`).
//! `--web DIR` (or `AOS_WEB_DIR`) serves a static web UI, such as the React build in `web/dist`, at `/`.

use anyhow::{Context, bail};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    aos_server::init_tracing();
    let mut config = aos_server::Config::from_env()?;

    let mut web = aos_server::web::web_dir_from_env();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--db" => config.db_path = args.next().context("--db needs a path")?.into(),
            "--addr" => {
                let a = args.next().context("--addr needs HOST:PORT")?;
                config.addr = a.parse().with_context(|| format!("bad address '{a}'"))?;
            }
            "--web" => web = Some(args.next().context("--web needs a folder")?.into()),
            "-h" | "--help" => {
                println!(
                    "aos-server [--db PATH] [--addr HOST:PORT] [--web DIR]\n\nEnv: AOS_DB, AOS_ADDR, AOS_WEB_DIR, AOS_CORS_ORIGINS, RUST_LOG"
                );
                return Ok(());
            }
            other => bail!("unknown argument '{other}' (try --help)"),
        }
    }
    aos_server::serve(config, move |router| match &web {
        Some(dir) => aos_server::web::with_web(router, dir),
        None => router,
    })
    .await
}
