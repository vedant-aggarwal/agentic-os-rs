//! Live end-to-end checks against the real CLIs on his login. Ignored by default (they use a few
//! tokens): `cargo test -p aos-work --test live -- --ignored --nocapture`.

use std::time::Duration;

use aos_work::types::*;
use aos_work::{Work, WorkConfig};

fn scratch(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("aos-work-live-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

async fn pong(provider: &str, model: &str) {
    let dir = scratch(provider);
    let cfg = WorkConfig::for_test(dir.join("work.sqlite"));
    let work = Work::start(cfg).unwrap();
    let mut events = work.subscribe();
    let t = work
        .create_thread(NewThread {
            folder: Some(dir.to_string_lossy().into()),
            provider: Some(provider.into()),
            model: Some(model.into()),
            prompt: "reply PONG".into(),
            ..Default::default()
        })
        .unwrap();
    let mut deltas = 0;
    let done = tokio::time::timeout(Duration::from_secs(120), async {
        loop {
            match events.recv().await {
                Ok(WorkEvent::ItemDelta { .. }) => deltas += 1,
                Ok(WorkEvent::TurnCompleted { turn, .. }) => return turn,
                _ => {}
            }
        }
    })
    .await
    .expect("turn finished in time");
    assert_eq!(done.status, TurnStatus::Completed, "{:?}", done.error);
    let d = work.thread(&t.id).unwrap();
    let reply = d.items.iter().filter(|i| i.kind == ItemKind::AssistantMessage).map(|i| i.text.clone()).collect::<String>();
    println!("{provider}: {reply:?} ({deltas} deltas, status {:?})", d.thread.status);
    assert!(reply.contains("PONG"));
    assert_eq!(d.thread.status, ThreadStatus::Done);
    work.stop_all();
    tokio::time::sleep(Duration::from_millis(500)).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn claude_pong() {
    pong("claude", "haiku").await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn codex_pong() {
    pong("codex", "").await;
}
