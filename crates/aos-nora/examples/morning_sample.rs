//! Makes a sample morning sheet from synthetic data (no model calls, nothing printed).
//!
//! ```text
//! $env:AOS_SEED = "crates/aos-core/seed.example.toml"   # synthetic habits and areas
//! cargo run -p aos-nora --example morning_sample -- <out-dir>
//! ```
//!
//! Writes `<out-dir>/sample-morning-sheet.pdf` and `.html`, and leaves a temp database whose path
//! it prints (the photo-intake check reuses the stored sheet through `plan_photo`).

use std::sync::Arc;

use aos_core::tasks::NewTask;
use aos_nora::engine::{Engine, TurnOutcome, TurnRequest, TurnUpdate};
use aos_nora::morning::{self, RunOptions};
use aos_nora::{Nora, NoraConfig};
use futures::future::BoxFuture;
use tokio::sync::mpsc;

/// Canned replies: the analysis for a plain turn, bullets for the web turn.
struct Canned;

impl Engine for Canned {
    fn run(&self, req: TurnRequest, _u: mpsc::UnboundedSender<TurnUpdate>) -> BoxFuture<'static, TurnOutcome> {
        let text = if req.web {
            "- Sample headline one about a city project, kept short and factual.\n- Sample headline two about the economy.\n- Sample headline three about a software release.\n- Sample headline four about a sports result.\n- Sample headline five about a scientific finding."
        } else {
            "DONE: Four of six tasks done, including the morning workout and the reading block.\nMISSED: The product page task (reason given: too big) and the supplier call (no reason logged).\nMOOD: Calm in the morning, energy 2 of 5 after lunch.\nPATTERN: Anything that needs a phone call slips to the next day; the written tasks all got done.\nTODAY: Split the product page into three small steps and do the first before 09:00."
        };
        Box::pin(async move { TurnOutcome { ok: true, texts: vec![text.into()], ..Default::default() } })
    }
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> anyhow::Result<()> {
    let out = std::path::PathBuf::from(std::env::args().nth(1).expect("usage: morning_sample <out-dir>"));
    std::fs::create_dir_all(&out)?;
    let db = std::env::temp_dir().join(format!("aos-sample-{}.db", aos_core::ids::new_id()));
    unsafe { std::env::set_var("AOS_PRINT_DIR", out.join("work")) };
    let mut cfg = NoraConfig::from_env(db.clone());
    cfg.grok.dir = None;
    cfg.owner = "Sam".into();
    let nora = Nora::new(cfg, Arc::new(Canned))?;
    let store = nora.core();
    let day = aos_core::time::local_today();
    let add = |title: &str, area: &str, sched: Option<&str>, parent: Option<&str>, due: chrono::NaiveDate| {
        store
            .add_task(
                "sample",
                NewTask {
                    title: title.into(),
                    area_id: Some(area.into()),
                    parent_id: parent.map(String::from),
                    due: Some(due),
                    scheduled: sched.map(|t| format!("{}T{t}", day.format("%Y-%m-%d"))),
                    ..Default::default()
                },
            )
            .unwrap()
    };
    let areas: Vec<String> = store.list_areas(false)?.into_iter().map(|a| a.id).collect();
    let pick = |i: usize| areas[i % areas.len()].clone();
    let g = add("Leg day workout", &pick(0), Some("06:30"), None, day);
    add("Warm up and mobility", &pick(0), None, Some(&g.id), day);
    add("Main lifts", &pick(0), None, Some(&g.id), day);
    add("Stretch and cool down", &pick(0), None, Some(&g.id), day);
    add("Read 20 pages", &pick(3), Some("21:00"), None, day);
    let p = add("Build the product page", &pick(1), Some("16:00"), None, day);
    add("Write the description", &pick(1), None, Some(&p.id), day);
    add("Pick photos", &pick(1), None, Some(&p.id), day);
    add("Publish and test on a phone", &pick(1), None, Some(&p.id), day);
    add("Call the supplier about the delivery", &pick(2), None, None, day - chrono::Duration::days(2));
    add("Revise the notes for the test", &pick(4), Some("18:30"), None, day);
    add("Clean the desk", &pick(5), None, None, day);
    store.set_alarm("sample", aos_core::time::now() + chrono::Duration::hours(2), "Water the plants", aos_core::model::AlarmKind::Reminder, None)?;
    let at = chrono::Utc::now().to_rfc3339();
    for (i, (name, text)) in [("Alex", "Can we move tomorrow's meeting to 11?"), ("Dana", "The invoice is attached, please confirm the amount.")].iter().enumerate() {
        aos_inbox::store::insert_item(
            store,
            "sample",
            &aos_inbox::store::NewItem {
                source: aos_inbox::store::Source::Whatsapp,
                account: "main".into(),
                external_id: format!("s{i}"),
                chat: format!("c{i}"),
                chat_name: name.to_string(),
                sender: format!("c{i}"),
                sender_name: name.to_string(),
                subject: String::new(),
                text: text.to_string(),
                at: at.clone(),
                from_me: false,
            },
        )?;
    }
    let r = morning::run(&nora, RunOptions { no_print: true, ..Default::default() }).await?;
    let pdf = std::path::PathBuf::from(r["pdf"].as_str().unwrap());
    std::fs::copy(&pdf, out.join("sample-morning-sheet.pdf"))?;
    std::fs::copy(pdf.with_extension("html"), out.join("sample-morning-sheet.html"))?;
    println!("{r:#}\nDB={}", db.display());
    Ok(())
}
