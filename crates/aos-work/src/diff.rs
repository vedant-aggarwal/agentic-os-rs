//! The diff view: what changed in the thread's folder, from git. Provider-neutral (T3 snapshots
//! each turn into hidden git refs; v1 shows the working tree against `HEAD`, plus new files).

use std::path::Path;
use std::process::Command;

use crate::types::{DiffFile, ThreadDiff};

const PATCH_KEEP: usize = 60_000;
const MAX_FILES: usize = 200;

fn git(folder: &str, args: &[&str]) -> Option<String> {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(folder).args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let out = cmd.output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn folder_diff(thread_id: &str, folder: &str) -> ThreadDiff {
    let base = ThreadDiff { thread_id: thread_id.into(), folder: folder.into(), is_git: false, files: vec![], note: None };
    if git(folder, &["rev-parse", "--is-inside-work-tree"]).map(|s| s.trim() == "true") != Some(true) {
        return ThreadDiff { note: Some("Not a git repository, so there is no diff to show.".into()), ..base };
    }
    let has_head = git(folder, &["rev-parse", "--verify", "HEAD"]).is_some();
    let mut files = Vec::new();
    if has_head {
        let numstat = git(folder, &["diff", "HEAD", "--numstat", "--find-renames"]).unwrap_or_default();
        let patch = git(folder, &["diff", "HEAD", "--find-renames", "--no-color"]).unwrap_or_default();
        let patches = split_patch(&patch);
        for line in numstat.lines().take(MAX_FILES) {
            let mut parts = line.splitn(3, '\t');
            let (a, r, path) = (parts.next().unwrap_or("0"), parts.next().unwrap_or("0"), parts.next().unwrap_or(""));
            let path = rename_target(path);
            let patch = patches.iter().find(|(p, _)| *p == path).map(|(_, t)| t.clone()).unwrap_or_default();
            let change = if patch.contains("\nnew file mode") {
                "added"
            } else if patch.contains("\ndeleted file mode") {
                "deleted"
            } else if patch.contains("\nrename from") {
                "renamed"
            } else {
                "modified"
            };
            files.push(DiffFile {
                path,
                added: a.parse().unwrap_or(0),
                removed: r.parse().unwrap_or(0),
                change: change.into(),
                patch: trim(patch),
            });
        }
    }
    let untracked = git(folder, &["ls-files", "--others", "--exclude-standard"]).unwrap_or_default();
    for path in untracked.lines().filter(|l| !l.is_empty()).take(MAX_FILES.saturating_sub(files.len())) {
        let content = std::fs::read(Path::new(folder).join(path)).ok();
        let (added, patch) = match content.as_deref().map(String::from_utf8_lossy) {
            Some(text) if !text.contains('\0') => {
                let n = text.lines().count() as u32;
                let body: String = text.lines().map(|l| format!("+{l}\n")).collect();
                (n, format!("--- /dev/null\n+++ b/{path}\n@@ -0,0 +1,{n} @@\n{body}"))
            }
            _ => (0, "Binary or unreadable file".into()),
        };
        files.push(DiffFile { path: path.into(), added, removed: 0, change: "untracked".into(), patch: trim(patch) });
    }
    ThreadDiff { is_git: true, files, ..base }
}

/// `a/{old => new}` and `old => new` forms from --numstat to the new path.
fn rename_target(path: &str) -> String {
    if let (Some(open), Some(close)) = (path.find('{'), path.find('}')) {
        let inner = &path[open + 1..close];
        let new = inner.split(" => ").nth(1).unwrap_or(inner);
        return format!("{}{}{}", &path[..open], new, &path[close + 1..]).replace("//", "/");
    }
    path.split(" => ").last().unwrap_or(path).to_string()
}

/// Split a multi-file patch into (path, patch) pairs by `diff --git` headers.
fn split_patch(patch: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for chunk in patch.split("diff --git ").filter(|c| !c.trim().is_empty()) {
        let header = chunk.lines().next().unwrap_or("");
        let path = header.split(" b/").last().unwrap_or("").trim().to_string();
        out.push((path, format!("diff --git {chunk}")));
    }
    out
}

fn trim(mut s: String) -> String {
    if s.len() > PATCH_KEEP {
        let mut cut = PATCH_KEEP;
        while !s.is_char_boundary(cut) {
            cut -= 1;
        }
        s.truncate(cut);
        s.push_str("\n… (trimmed)");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rename_paths() {
        assert_eq!(rename_target("src/{a.rs => b.rs}"), "src/b.rs");
        assert_eq!(rename_target("old.txt => new.txt"), "new.txt");
        assert_eq!(rename_target("plain.rs"), "plain.rs");
    }

    #[test]
    fn splits_patches() {
        let p = "diff --git a/x b/x\n+1\ndiff --git a/y/z b/y/z\n-2\n";
        let s = split_patch(p);
        assert_eq!(s.len(), 2);
        assert_eq!(s[1].0, "y/z");
    }

    #[test]
    fn non_git_folder() {
        let dir = std::env::temp_dir().join(format!("aos-work-nogit-{}", crate::store::new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let d = folder_diff("t", dir.to_str().unwrap());
        // The temp dir may sit inside a repo on some machines; only check the shape.
        if !d.is_git {
            assert!(d.note.is_some());
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
