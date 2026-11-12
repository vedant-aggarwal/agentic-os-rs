//! The enrichment queue. Stages run in order per item: `fetch` (a link's page: title,
//! description, image, readable text) -> `thumb` (a <= 512 px JPEG and the dominant colour) ->
//! `tag` (Claude's title, description, summary and tags). Sensitive items never get `tag`.
//!
//! The workers (server and Nora) take jobs with [`Store::mind_next_job`], do the slow part
//! outside the database, store results with the `mind_apply_*` calls and close the job with
//! [`Store::mind_job_finish`], which queues the next stage.

use chrono::Duration;
use rusqlite::{OptionalExtension, params};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{clean_tag, fts_sync};
use crate::error::Result;
use crate::mission::{row, s};
use crate::store::{ChangeOp, Store, Tx};
use crate::time;

/// Give up on a stage after this many tries.
pub const MAX_TRIES: i64 = 3;
/// Bumped when the tag prompt changes, so old results can be re-run.
pub const PROMPT_VERSION: &str = "mind-tag-1";

pub(crate) fn enqueue(tx: &Tx<'_>, id: &str, stage: &str) -> Result<()> {
    tx.execute(
        "INSERT INTO mind_job (item_id, stage, state, tries, run_after) VALUES (?1, ?2, 'queued', 0, ?3)
         ON CONFLICT(item_id, stage) DO UPDATE SET state = 'queued', tries = 0, error = NULL, run_after = excluded.run_after",
        params![id, stage, tx.now()],
    )?;
    Ok(())
}

/// What a link's page gave us.
#[derive(Debug, Clone, Default, Deserialize, serde::Serialize)]
pub struct Fetched {
    pub title: Option<String>,
    pub description: Option<String>,
    pub site_name: Option<String>,
    pub image_url: Option<String>,
    pub body_text: Option<String>,
    /// The final URL after redirects.
    pub url: Option<String>,
    /// og:type video etc. turns a link into a video card.
    pub video: bool,
}

/// What Claude read from an item.
#[derive(Debug, Clone, Default, Deserialize, serde::Serialize)]
pub struct Reading {
    pub title: Option<String>,
    pub description: Option<String>,
    pub summary: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

fn clip(s: Option<String>, max: usize) -> Option<String> {
    s.map(|v| v.trim().to_string()).filter(|v| !v.is_empty()).map(|v| if v.chars().count() > max { v.chars().take(max).collect() } else { v })
}

impl Store {
    /// The oldest due job in one of `stages`, marked as running (status shows on the card).
    pub fn mind_next_job(&self, stages: &[&str]) -> Result<Option<(String, String)>> {
        if stages.is_empty() {
            return Ok(None);
        }
        self.write("mind-worker", |tx| {
            let list = stages.iter().map(|st| format!("'{}'", st.replace('\'', ""))).collect::<Vec<_>>().join(",");
            let job: Option<(String, String)> = tx
                .query_row(
                    &format!(
                        "SELECT j.item_id, j.stage FROM mind_job j JOIN mind_item i ON i.id = j.item_id
                         WHERE j.state = 'queued' AND j.stage IN ({list}) AND j.run_after <= ?1 AND i.deleted_at IS NULL
                         ORDER BY j.run_after LIMIT 1"
                    ),
                    [tx.now()],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            if let Some((id, stage)) = &job {
                let status = if stage == "fetch" { "fetching" } else { "reading" };
                tx.execute("UPDATE mind_job SET state = 'running', tries = tries + 1 WHERE item_id = ?1 AND stage = ?2", params![id, stage])?;
                tx.execute("UPDATE mind_item SET status = ?2 WHERE id = ?1", params![id, status])?;
            }
            Ok(job)
        })
    }

    /// Jobs left in `running` by a worker that stopped go back to the queue.
    pub fn mind_requeue_running(&self) -> Result<usize> {
        self.write("mind-worker", |tx| Ok(tx.execute("UPDATE mind_job SET state = 'queued' WHERE state = 'running'", [])?))
    }

    /// Close a job. Success queues the next stage (or marks the item ready); failure retries
    /// later with a backoff, then gives up and moves on so the item still gets its other stages.
    pub fn mind_job_finish(&self, id: &str, stage: &str, error: Option<&str>) -> Result<()> {
        self.write("mind-worker", |tx| {
            let (tries, sensitive, kind, has_img): (i64, i64, String, bool) = tx.query_row(
                "SELECT j.tries, i.sensitive, i.kind, (i.thumb_asset_id IS NULL AND (i.asset_id IS NOT NULL OR i.image_url IS NOT NULL))
                 FROM mind_job j JOIN mind_item i ON i.id = j.item_id WHERE j.item_id = ?1 AND j.stage = ?2",
                params![id, stage],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )?;
            let _ = kind;
            if let Some(e) = error {
                if tries < MAX_TRIES {
                    let later = time::ts(time::now() + Duration::seconds(30 * 4_i64.pow(tries as u32)));
                    tx.execute("UPDATE mind_job SET state = 'queued', error = ?3, run_after = ?4 WHERE item_id = ?1 AND stage = ?2", params![id, stage, e, later])?;
                    return Ok(());
                }
                tx.execute("UPDATE mind_job SET state = 'failed', error = ?3 WHERE item_id = ?1 AND stage = ?2", params![id, stage, e])?;
                tx.execute("UPDATE mind_item SET error = ?2 WHERE id = ?1", params![id, format!("{stage}: {e}")])?;
            } else {
                tx.execute("UPDATE mind_job SET state = 'done', error = NULL WHERE item_id = ?1 AND stage = ?2", params![id, stage])?;
            }
            let next = match stage {
                "fetch" if has_img => Some("thumb"),
                "fetch" | "thumb" if sensitive == 0 => Some("tag"),
                _ => None,
            };
            match next {
                Some(n) => enqueue(tx, id, n)?,
                None => {
                    let failed_all: i64 = tx.query_row("SELECT count(*) FROM mind_job WHERE item_id = ?1 AND state = 'done'", [id], |r| r.get(0))?;
                    let status = if failed_all == 0 && error.is_some() { "failed" } else { "ready" };
                    tx.execute("UPDATE mind_item SET status = ?2, updated_at = ?3 WHERE id = ?1", params![id, status, tx.now()])?;
                }
            }
            tx.record("mind_item", id, ChangeOp::Upsert)?;
            Ok(())
        })
    }

    /// Store what a link's page said. Never overwrites a title he gave.
    pub fn mind_apply_fetch(&self, id: &str, f: &Fetched) -> Result<()> {
        self.write("mind-worker", |tx| {
            tx.execute(
                "UPDATE mind_item SET title = coalesce(title, ?2), description = coalesce(description, ?3), site_name = coalesce(site_name, ?4),
                    image_url = coalesce(image_url, ?5), body_text = coalesce(?6, body_text),
                    kind = CASE WHEN ?7 = 1 AND kind = 'link' THEN 'video' ELSE kind END, updated_at = ?8 WHERE id = ?1",
                params![
                    id,
                    clip(f.title.clone(), 200),
                    clip(f.description.clone(), 600),
                    clip(f.site_name.clone(), 80),
                    clip(f.image_url.clone(), 2000),
                    clip(f.body_text.clone(), 60_000),
                    f.video as i64,
                    tx.now()
                ],
            )?;
            super::links::resolve_dangling(tx, id)?;
            fts_sync(tx, id)?;
            tx.record("mind_item", id, ChangeOp::Upsert)?;
            Ok(())
        })
    }

    /// Store the downloaded original (for links with an image) and the thumbnail.
    pub fn mind_apply_thumb(&self, id: &str, asset_id: Option<&str>, thumb_id: &str, w: i64, h: i64, color: Option<&str>) -> Result<()> {
        self.write("mind-worker", |tx| {
            tx.execute(
                "UPDATE mind_item SET asset_id = coalesce(asset_id, ?2), thumb_asset_id = ?3, width = coalesce(width, ?4), height = coalesce(height, ?5),
                    color = coalesce(color, ?6), updated_at = ?7 WHERE id = ?1",
                params![id, asset_id, thumb_id, w, h, color, tx.now()],
            )?;
            tx.record("mind_item", id, ChangeOp::Upsert)?;
            Ok(())
        })
    }

    /// Store Claude's reading. His title and his tags win; tags he removed are never re-added.
    pub fn mind_apply_reading(&self, id: &str, r: &Reading, model: &str) -> Result<()> {
        self.write("mind-worker", |tx| {
            tx.execute(
                "UPDATE mind_item SET title = coalesce(title, ?2), description = ?3, summary = ?4, model = ?5, prompt_version = ?6, updated_at = ?7 WHERE id = ?1",
                params![id, clip(r.title.clone(), 120), clip(r.description.clone(), 600), clip(r.summary.clone(), 1200), model, PROMPT_VERSION, tx.now()],
            )?;
            for t in r.tags.iter().filter_map(|t| clean_tag(t)).take(8) {
                tx.execute("INSERT OR IGNORE INTO mind_tag (item_id, tag, by) VALUES (?1, ?2, 'claude')", params![id, t])?;
            }
            super::links::resolve_dangling(tx, id)?;
            fts_sync(tx, id)?;
            tx.record("mind_item", id, ChangeOp::Upsert)?;
            Ok(())
        })
    }

    /// What a worker needs for one item: kind, raw text, link, page text excerpt, the
    /// original or thumbnail asset ids and the tags already in use.
    pub fn mind_job_input(&self, id: &str) -> Result<Value> {
        let item = self.read(|c| {
            row(
                c,
                "SELECT id, kind, raw, title, description, source_url, source_domain, site_name, image_url, asset_id, thumb_asset_id, sensitive,
                    substr(coalesce(body_text, ''), 1, 4000) AS body_excerpt FROM mind_item WHERE id = ?1",
                &[s(id)],
                "mind_item",
                id,
            )
        })?;
        let known: Vec<String> = self.mind_tags(60)?.into_iter().filter_map(|t| t["tag"].as_str().map(String::from)).collect();
        let mut v = item;
        v["known_tags"] = json!(known);
        Ok(v)
    }

    /// Queue Claude again for items that were never read (or every item with `all`).
    pub fn mind_retag(&self, by: &str, all: bool) -> Result<usize> {
        self.write(by, |tx| {
            let ids: Vec<String> = {
                let sql = if all {
                    "SELECT id FROM mind_item WHERE deleted_at IS NULL AND sensitive = 0"
                } else {
                    "SELECT id FROM mind_item WHERE deleted_at IS NULL AND sensitive = 0 AND description IS NULL"
                };
                let mut st = tx.prepare(sql)?;
                st.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?
            };
            for id in &ids {
                enqueue(tx, id, "tag")?;
            }
            Ok(ids.len())
        })
    }
}
