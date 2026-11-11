//! Middle column: the thread. Header, plan, timeline (messages, tool calls collapsed, approval
//! cards), composer.

use aos_work::types::*;
use dioxus::prelude::*;

use super::{StatusPill, WI, WIcon, WorkSheet, model_label};
use crate::pages::work::{WorkUi, api};

enum Entry {
    Item(Item),
    Approval(Approval),
}

impl Entry {
    fn at(&self) -> Millis {
        match self {
            Entry::Item(i) => i.created_at,
            Entry::Approval(a) => a.created_at,
        }
    }
}

#[component]
pub fn ThreadView() -> Element {
    let mut ui = use_context::<WorkUi>();
    let detail = ui.detail.read().clone();
    let has_selection = ui.selected.read().is_some();

    // Keep the newest message in view while streaming.
    use_effect(move || {
        let _ = ui.detail.read();
        let _ = document::eval(
            "requestAnimationFrame(() => { const el = document.querySelector('.wk-timeline'); \
             if (el && el.scrollHeight - el.scrollTop - el.clientHeight < 260) el.scrollTop = el.scrollHeight; });",
        );
    });

    let Some(d) = detail else {
        return rsx! {
            main { class: "wk-thread wk-thread-empty",
                if has_selection {
                    p { class: "wk-muted", "Loading…" }
                } else {
                    div { class: "wk-empty",
                        h2 { "Pick a thread, or start one" }
                        p { class: "wk-muted", "Claude and Codex sessions run on the PC. You can approve, steer and stop them from here." }
                        div { class: "wk-actions",
                            button { class: "wk-btn wk-btn-primary", r#type: "button", onclick: move |_| ui.sheet.set(WorkSheet::NewThread), "New thread" }
                            button { class: "wk-btn", r#type: "button", onclick: move |_| ui.sheet.set(WorkSheet::Launch), "Launch several" }
                        }
                    }
                }
            }
        };
    };

    let t = d.thread.clone();
    let mut entries: Vec<Entry> = d.items.iter().cloned().map(Entry::Item).collect();
    entries.extend(d.approvals.iter().cloned().map(Entry::Approval));
    entries.sort_by_key(|e| e.at());
    let project = d.project.as_ref().map(|p| p.name.clone()).unwrap_or_default();
    let tid = t.id.clone();
    let tid_stop = t.id.clone();
    let changed = ui.diff.read().as_ref().map(|d| d.files.len()).unwrap_or(0);

    rsx! {
        main { class: "wk-thread",
            header { class: "wk-thread-head",
                button { class: "wk-icon-btn wk-back", r#type: "button", aria_label: "Back to threads", onclick: move |_| ui.select(None),
                    WI { name: WIcon::Back, size: 20 }
                }
                div { class: "wk-thread-title",
                    h2 { "{t.title}" }
                    div { class: "wk-thread-meta",
                        StatusPill { status: t.status, pending: t.pending_approvals }
                        span { "{project}" }
                        span { "{model_label(t.provider, &t.model)}" }
                        span { "{t.runtime_mode.label()}" }
                        if t.interaction_mode == InteractionMode::Plan { span { class: "wk-tag", "Plan" } }
                    }
                }
                div { class: "wk-head-actions",
                    if t.status.is_active() {
                        button { class: "wk-btn wk-btn-quiet", r#type: "button", title: "Stop the turn",
                            onclick: move |_| { let id = tid_stop.clone(); spawn(async move { let _ = api::work_interrupt(id).await; }); },
                            WI { name: WIcon::Stop, size: 14 }
                            "Stop"
                        }
                    }
                    button {
                        class: "wk-btn wk-btn-quiet wk-diff-toggle",
                        r#type: "button",
                        onclick: move |_| { let open = *ui.show_diff.peek(); ui.show_diff.set(!open); if !open { ui.load_diff(); } },
                        WI { name: WIcon::Diff, size: 14 }
                        if changed > 0 { "Diff {changed}" } else { "Diff" }
                    }
                }
            }
            div { class: "wk-timeline",
                if !d.plan.is_empty() {
                    PlanCard { steps: d.plan.clone() }
                }
                for e in entries {
                    match e {
                        Entry::Item(i) => rsx! { ItemView { key: "{i.id}", item: i } },
                        Entry::Approval(a) => rsx! { ApprovalCard { key: "{a.id}", approval: a } },
                    }
                }
                if t.status == ThreadStatus::Working || t.status == ThreadStatus::Starting {
                    div { class: "wk-working", span { class: "wk-spinner" } if t.status == ThreadStatus::Starting { "Starting {t.provider.label()}…" } else { "Working…" } }
                }
            }
            Composer { thread_id: tid, busy: t.status.is_active() }
        }
    }
}

#[component]
fn ItemView(item: Item) -> Element {
    match item.kind {
        ItemKind::UserMessage => rsx! {
            div { class: "wk-msg wk-msg-user", div { class: "wk-bubble", "{item.text}" } }
        },
        ItemKind::AssistantMessage => rsx! {
            div { class: "wk-msg wk-msg-agent",
                div { class: "wk-text", "{item.text}"
                    if item.status == ItemStatus::Running { span { class: "wk-caret" } }
                }
            }
        },
        ItemKind::Reasoning => rsx! {
            details { class: "wk-fold wk-reason",
                summary { "Thinking" }
                pre { class: "wk-pre", "{item.text}" }
            }
        },
        ItemKind::ToolCall => {
            let input = item.input_pretty();
            rsx! {
                details { class: "wk-fold wk-tool", "data-status": item.status.as_str(),
                    summary {
                        span { class: "wk-tool-dot" }
                        span { class: "wk-tool-title", "{item.title}" }
                    }
                    if !input.is_empty() { pre { class: "wk-pre", "{input}" } }
                    if !item.text.is_empty() { pre { class: "wk-pre wk-out", "{item.text}" } }
                }
            }
        }
        ItemKind::Plan => rsx! {
            div { class: "wk-card wk-plan-text",
                div { class: "wk-card-label", "Plan" }
                div { class: "wk-text", "{item.text}" }
            }
        },
        ItemKind::Error => rsx! { div { class: "wk-error", "{item.text}" } },
        ItemKind::System => rsx! { div { class: "wk-system", "{item.text}" } },
    }
}

#[component]
fn PlanCard(steps: Vec<PlanStep>) -> Element {
    let done = steps.iter().filter(|s| s.status == "completed").count();
    rsx! {
        div { class: "wk-card wk-plan",
            div { class: "wk-card-label", "Plan " span { class: "wk-muted", "{done}/{steps.len()}" } }
            ol {
                for (i, s) in steps.iter().enumerate() {
                    li { key: "{i}", "data-status": "{s.status}", "{s.step}" }
                }
            }
        }
    }
}

#[component]
fn ApprovalCard(approval: Approval) -> Element {
    let mut busy = use_signal(|| false);
    let kind = match approval.kind {
        ApprovalKind::Command => "Command",
        ApprovalKind::FileChange => "File change",
        ApprovalKind::Tool => "Tool",
        ApprovalKind::Plan => "Plan",
    };
    if approval.state != ApprovalState::Pending {
        let verb = match approval.state {
            ApprovalState::Approved => "Approved",
            ApprovalState::Denied => "Denied",
            _ => "Expired",
        };
        return rsx! { div { class: "wk-resolved", "data-state": approval.state.as_str(), "{verb}: {approval.title}" } };
    }
    let decide = move |d: Decision| {
        let id = approval.id.clone();
        move |_| {
            let id = id.clone();
            busy.set(true);
            spawn(async move {
                let _ = api::work_decide(id, d).await;
                busy.set(false);
            });
        }
    };
    let (yes, always) = if approval.kind == ApprovalKind::Plan { ("Build it", "Build, auto-edit") } else { ("Approve", "Always") };
    rsx! {
        div { class: "wk-approval", role: "group", aria_label: "Approval needed",
            div { class: "wk-card-label", "Needs you · {kind}" }
            div { class: "wk-approval-title", "{approval.title}" }
            if !approval.detail.is_empty() { pre { class: "wk-pre", "{approval.detail}" } }
            div { class: "wk-approval-actions",
                button { class: "wk-btn wk-btn-primary", r#type: "button", disabled: busy(), onclick: decide(Decision::Approve), "{yes}" }
                button { class: "wk-btn", r#type: "button", disabled: busy(), onclick: decide(Decision::ApproveSession), "{always}" }
                button { class: "wk-btn wk-btn-danger", r#type: "button", disabled: busy(), onclick: decide(Decision::Deny), "Deny" }
            }
        }
    }
}

#[component]
fn Composer(thread_id: String, busy: bool) -> Element {
    let mut text = use_signal(String::new);
    let mut sending = use_signal(|| false);
    let mut ui = use_context::<WorkUi>();
    let mut send = move |id: String| {
        let msg = text.peek().trim().to_string();
        if msg.is_empty() || *sending.peek() {
            return;
        }
        sending.set(true);
        spawn(async move {
            match api::work_send(id, msg).await {
                Ok(_) => text.set(String::new()),
                Err(e) => ui.error.set(Some(e.to_string())),
            }
            sending.set(false);
        });
    };
    let id_key = thread_id.clone();
    let id_btn = thread_id.clone();
    rsx! {
        div { class: "wk-composer",
            textarea {
                class: "wk-input",
                rows: "1",
                placeholder: if busy { "Queue a follow-up…" } else { "Message the agent…" },
                value: "{text}",
                oninput: move |e| text.set(e.value()),
                onkeydown: move |e: KeyboardEvent| {
                    if e.key() == Key::Enter && !e.modifiers().shift() {
                        e.prevent_default();
                        send(id_key.clone());
                    }
                },
            }
            button { class: "wk-send", r#type: "button", aria_label: "Send", disabled: sending() || text.read().trim().is_empty(),
                onclick: move |_| send(id_btn.clone()),
                WI { name: WIcon::Send, size: 18 }
            }
        }
    }
}
