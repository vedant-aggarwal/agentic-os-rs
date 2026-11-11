//! Left column: header, project filter, threads grouped Needs you / Working / Recent.

use aos_work::types::*;
use dioxus::prelude::*;

use super::{StatusPill, WI, WIcon, WorkSheet, ago, model_label};
use crate::pages::work::WorkUi;

#[component]
pub fn ThreadList() -> Element {
    let mut ui = use_context::<WorkUi>();
    let filter = ui.filter.read().clone();
    let threads: Vec<ThreadSummary> = ui
        .threads
        .read()
        .iter()
        .filter(|t| filter.as_ref().is_none_or(|f| &t.project_id == f))
        .cloned()
        .collect();
    let needs: Vec<_> = threads.iter().filter(|t| t.status == ThreadStatus::NeedsYou || t.pending_approvals > 0).cloned().collect();
    let working: Vec<_> = threads
        .iter()
        .filter(|t| matches!(t.status, ThreadStatus::Working | ThreadStatus::Starting) && t.pending_approvals == 0)
        .cloned()
        .collect();
    let rest: Vec<_> = threads
        .iter()
        .filter(|t| !t.status.is_active() && t.pending_approvals == 0)
        .cloned()
        .collect();
    let projects = ui.projects.read().clone();
    let live = *ui.live.read();
    let error = ui.error.read().clone();

    rsx! {
        aside { class: "wk-side",
            header { class: "wk-side-head",
                div { class: "wk-title-row",
                    h1 { class: "wk-title", "Work" }
                    span { class: if live { "wk-live on" } else { "wk-live" }, title: if live { "Live" } else { "Reconnecting" } }
                }
                div { class: "wk-actions",
                    button { class: "wk-btn wk-btn-primary", r#type: "button", onclick: move |_| ui.sheet.set(WorkSheet::NewThread),
                        WI { name: WIcon::Plus, size: 16 }
                        "New thread"
                    }
                    button { class: "wk-btn", r#type: "button", onclick: move |_| ui.sheet.set(WorkSheet::Launch),
                        WI { name: WIcon::Stack, size: 16 }
                        "Launch several"
                    }
                }
            }
            if let Some(err) = error {
                p { class: "wk-banner", "{err}" }
            }
            div { class: "wk-chips", role: "tablist", aria_label: "Projects",
                button {
                    class: if filter.is_none() { "wk-chip on" } else { "wk-chip" },
                    r#type: "button",
                    onclick: move |_| ui.filter.set(None),
                    "All"
                }
                for p in projects {
                    {
                        let on = filter.as_deref() == Some(p.id.as_str());
                        let id = p.id.clone();
                        rsx! {
                            button {
                                key: "{p.id}",
                                class: if on { "wk-chip on" } else { "wk-chip" },
                                r#type: "button",
                                title: "{p.folder}",
                                onclick: move |_| ui.filter.set(Some(id.clone())),
                                "{p.name}"
                            }
                        }
                    }
                }
            }
            div { class: "wk-groups",
                if threads.is_empty() {
                    div { class: "wk-empty-list",
                        p { "No threads yet." }
                        p { class: "wk-muted", "Start one, or launch several at once." }
                    }
                }
                Group { label: "Needs you", threads: needs }
                Group { label: "Working", threads: working }
                Group { label: "Recent", threads: rest }
            }
        }
    }
}

#[component]
fn Group(label: &'static str, threads: Vec<ThreadSummary>) -> Element {
    if threads.is_empty() {
        return rsx! {};
    }
    rsx! {
        section { class: "wk-group",
            h2 { class: "wk-group-label", "{label}" span { class: "wk-count", "{threads.len()}" } }
            for t in threads {
                ThreadRow { key: "{t.id}", thread: t }
            }
        }
    }
}

#[component]
fn ThreadRow(thread: ThreadSummary) -> Element {
    let ui = use_context::<WorkUi>();
    let selected = ui.selected.read().as_deref() == Some(thread.id.as_str());
    let project = ui.project(&thread.project_id).map(|p| p.name).unwrap_or_default();
    let id = thread.id.clone();
    let preview = if let Some(err) = thread.last_error.as_ref().filter(|_| thread.status == ThreadStatus::Error) {
        err.clone()
    } else {
        thread.preview.clone()
    };
    rsx! {
        button {
            class: if selected { "wk-row on" } else { "wk-row" },
            "data-status": thread.status.as_str(),
            r#type: "button",
            onclick: move |_| ui.select(Some(id.clone())),
            div { class: "wk-row-top",
                span { class: "wk-row-title", "{thread.title}" }
                span { class: "wk-row-time", "{ago(thread.updated_at)}" }
            }
            if !preview.is_empty() {
                p { class: "wk-row-preview", "{preview}" }
            }
            div { class: "wk-row-meta",
                StatusPill { status: thread.status, pending: thread.pending_approvals }
                span { class: "wk-muted", "{project}" }
                span { class: "wk-muted", "{model_label(thread.provider, &thread.model)}" }
            }
        }
    }
}
