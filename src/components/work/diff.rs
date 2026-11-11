//! Right column (desktop) or overlay (phone): files changed in the thread's folder.

use dioxus::prelude::*;

use super::{WI, WIcon};
use crate::pages::work::WorkUi;

#[component]
pub fn DiffPanel() -> Element {
    let mut ui = use_context::<WorkUi>();
    let diff = ui.diff.read().clone();
    let mut open_file = use_signal(|| None::<String>);
    let has_thread = ui.selected.read().is_some();

    rsx! {
        section { class: "wk-diff", aria_label: "Changed files",
            header { class: "wk-diff-head",
                h2 { "Changes" }
                if let Some(d) = &diff {
                    span { class: "wk-muted", if d.files.len() == 1 { "1 file" } else { "{d.files.len()} files" } }
                }
                button { class: "wk-icon-btn wk-diff-close", r#type: "button", aria_label: "Close diff", onclick: move |_| ui.show_diff.set(false),
                    WI { name: WIcon::Close, size: 18 }
                }
            }
            div { class: "wk-diff-body",
                match diff {
                    None if has_thread => rsx! { p { class: "wk-muted", "Loading…" } },
                    None => rsx! { p { class: "wk-muted", "Open a thread to see what it changed." } },
                    Some(d) if !d.is_git => rsx! { p { class: "wk-muted", {d.note.clone().unwrap_or_default()} } },
                    Some(d) if d.files.is_empty() => rsx! { p { class: "wk-muted", "No changes in this folder." } },
                    Some(d) => rsx! {
                        for f in d.files {
                            {
                                let open = open_file.read().as_deref() == Some(f.path.as_str());
                                let path = f.path.clone();
                                rsx! {
                                    div { key: "{f.path}", class: "wk-file",
                                        button { class: "wk-file-row", r#type: "button",
                                            onclick: move |_| { let p = path.clone(); let cur = open_file.peek().clone(); open_file.set(if cur.as_deref() == Some(p.as_str()) { None } else { Some(p) }); },
                                            span { class: "wk-file-change", "data-change": "{f.change}" }
                                            span { class: "wk-file-path", "{f.path}" }
                                            span { class: "wk-add", "+{f.added}" }
                                            span { class: "wk-del", "−{f.removed}" }
                                        }
                                        if open {
                                            pre { class: "wk-patch",
                                                for (i, line) in f.patch.lines().enumerate() {
                                                    span {
                                                        key: "{i}",
                                                        class: if line.starts_with('+') && !line.starts_with("+++") { "a" } else if line.starts_with('-') && !line.starts_with("---") { "d" } else if line.starts_with("@@") { "h" } else { "" },
                                                        "{line}\n"
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    },
                }
            }
        }
    }
}
