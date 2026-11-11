//! Checks the photo intake with the real Claude CLI against a database that holds a sheet made
//! by `morning_sample`.
//!
//! ```text
//! $env:AOS_SEED = "crates/aos-core/seed.example.toml"
//! cargo run -p aos-nora --example morning_photo -- <db-path> <photo.jpg>
//! ```

use std::sync::Arc;

use aos_nora::engine::ClaudeCli;
use aos_nora::{Nora, NoraConfig};

#[tokio::main(flavor = "multi_thread")]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let (db, photo) = (args.next().expect("usage: morning_photo <db> <photo>"), args.next().expect("photo"));
    let mut cfg = NoraConfig::from_env(db.into());
    cfg.grok.dir = None;
    cfg.owner = "Sam".into();
    let engine = Arc::new(ClaudeCli { bin: cfg.claude_bin.clone(), cwd: cfg.workdir.clone() });
    let nora = Nora::new(cfg, engine)?;
    let before: Vec<_> = nora.core().list_tasks(&Default::default())?.iter().map(|t| format!("{} [{}]", t.task.title, t.task.status)).collect();
    let out = aos_nora::morning::intake::process(&nora, std::fs::read(photo)?, None).await?;
    println!("{out:#}");
    println!("BEFORE {before:#?}");
    let all = nora.core().list_tasks(&aos_core::tasks::TaskFilter { status: aos_core::tasks::StatusFilter::All, ..Default::default() })?;
    for t in all {
        println!("AFTER  {} [{}] due={:?} reason={:?} notes={:?}", t.task.title, t.task.status, t.task.due, t.task.skip_reason, t.task.notes);
    }
    Ok(())
}
