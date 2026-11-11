//! `aos-server`: the Agentic OS API on its own (no web UI).
//!
//! ```text
//! aos-server [--db PATH] [--addr HOST:PORT]
//! ```
//! Defaults: `%USERPROFILE%\.aos\aos.db` (or `AOS_DB`) and `127.0.0.1:8787` (or `AOS_ADDR`).

use anyhow::{Context, bail};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    aos_server::init_tracing();
    let mut config = aos_server::Config::from_env()?;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--db" => config.db_path = args.next().context("--db needs a path")?.into(),
            "--addr" => {
                let a = args.next().context("--addr needs HOST:PORT")?;
                config.addr = a.parse().with_context(|| format!("bad address '{a}'"))?;
            }
            "-h" | "--help" => {
                println!(
                    "aos-server [--db PATH] [--addr HOST:PORT]\n\nEnv: AOS_DB, AOS_ADDR, RUST_LOG"
                );
                return Ok(());
            }
            other => bail!("unknown argument '{other}' (try --help)"),
        }
    }
    aos_server::serve(config, |router| router).await
}
