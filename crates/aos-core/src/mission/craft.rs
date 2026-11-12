//! The Craft sync seam (research/mission-control.md section 3). Not wired yet.
//!
//! What exists now:
//! - `mc_node.craft_id`, `craft_base`, `craft_seen_at` on every block, and the `craft_link`
//!   allowlist table (space -> Craft document id).
//! - [`base_hash`]: the hash both sides agree on after a sync.
//! - [`CraftSync`]: what a sync worker must provide. [`NoCraft`] is the stand-in.
//!
//! The worker (later) runs on the server only, polls (Craft has no webhooks), does a block-level
//! three-way merge with `craft_base`, lets a human edit beat Nora, waits 24 hours before a
//! delete reaches Craft, and refuses any document id not in `craft_link`. Layout, dream paths,
//! goal measures, rules stats, evidence and proposals never sync.

use sha2::{Digest, Sha256};

use crate::error::Result;
use crate::store::Store;

/// What one sync pass did.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct SyncReport {
    pub pulled: usize,
    pub pushed: usize,
    pub conflicts: usize,
    pub skipped: usize,
}

/// A Craft sync engine for the three spaces (viz, ver, newme).
pub trait CraftSync: Send + Sync {
    /// Read the mapped document for `space` and apply Craft-side changes to `mc_node`.
    fn pull(&self, store: &Store, space: &str) -> Result<SyncReport>;
    /// Send app-side changes of these node ids to Craft.
    fn push(&self, store: &Store, node_ids: &[String]) -> Result<SyncReport>;
}

/// Sync turned off: does nothing, reports nothing.
pub struct NoCraft;

impl CraftSync for NoCraft {
    fn pull(&self, _: &Store, _: &str) -> Result<SyncReport> {
        Ok(SyncReport::default())
    }
    fn push(&self, _: &Store, ids: &[String]) -> Result<SyncReport> {
        Ok(SyncReport { skipped: ids.len(), ..Default::default() })
    }
}

/// Hash of (text, style, parent Craft id, previous sibling Craft id): the `craft_base` value.
pub fn base_hash(text: &str, style: &str, parent: Option<&str>, prev: Option<&str>) -> String {
    let mut h = Sha256::new();
    for part in [text, style, parent.unwrap_or(""), prev.unwrap_or("")] {
        h.update(part.as_bytes());
        h.update([0u8]);
    }
    h.finalize().iter().take(16).map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_changes_with_any_part() {
        let a = base_hash("x", "body", None, None);
        assert_ne!(a, base_hash("x", "h1", None, None));
        assert_ne!(a, base_hash("x", "body", Some("p"), None));
        assert_eq!(a, base_hash("x", "body", None, None));
        assert_eq!(NoCraft.push(&Store::open_temporary().unwrap(), &["a".into()]).unwrap().skipped, 1);
    }
}
