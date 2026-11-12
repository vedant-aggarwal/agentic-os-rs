//! Version I want to be: identity statements and goals by horizon, as swipeable cards per
//! section. Progress comes from real evidence (linked tasks done, habit logs, journal links).

use dioxus::prelude::*;
use serde_json::{Value, json};

use super::data::{Mc, hue_of, section_name, short_day, txt};
use crate::components::{Icon, IconName};
use crate::live::Live;

const SECTIONS: [&str; 5] = ["physical", "mental", "monetary", "social", "other"];

#[component]
pub fn VersionView() -> Element {
    let mut mc = use_context::<Mc>();
    let live = use_context::<Live>();
    use_effect(move || {
        if live.conn.read().is_some() {
            spawn(async move { mc.load_version().await });
        }
    });
    let horizon = (mc.horizon)();
    let goals = (mc.goals)();
    let statements = (mc.statements)();
    let mut new_statement = use_signal(String::new);

    rsx! {
        section { class: "mc-version",
            h2 { class: "mc-h", "Who I'm becoming" }
            p { class: "mc-sub", "Each statement counts the days you acted on it in the last two weeks." }
            if goals.is_some() { div { class: "mc-deck",
                for s in statements.iter() {
                    StatementCard { key: "{txt(s, \"id\")}", s: s.clone() }
                }
                form { class: "mc-card mc-card-add",
                    onsubmit: move |e| {
                        e.prevent_default();
                        let t = new_statement.peek().trim().to_string();
                        if t.is_empty() { return; }
                        let id = mc.new_id();
                        mc.statements.write().push(json!({ "id": id, "text": t, "section": "mental", "votes_14d": 0 }));
                        mc.write("mission_identity_add", json!({ "id": id, "text": t }));
                        new_statement.set(String::new());
                    },
                    span { class: "mc-card-label", "New statement" }
                    textarea { rows: "3", placeholder: "I am someone who…", value: "{new_statement}", oninput: move |e| new_statement.set(e.value()) }
                    button { class: "btn", r#type: "submit", disabled: new_statement().trim().is_empty(), "Add" }
                }
            } }

            div { class: "mc-horizons", role: "tablist",
                for (h, label, span) in [("short", "Short", "0–3 months"), ("mid", "Mid", "3–12 months"), ("long", "Long", "1 year +")] {
                    button { key: "{h}", r#type: "button", role: "tab", class: if horizon == h { "on" }, onclick: move |_| mc.horizon.set(h),
                        b { "{label}" } span { "{span}" }
                    }
                }
            }
            match goals {
                None => rsx! { p { class: "mc-empty", if live.paired() { "Loading…" } else { "Pair this device to see your goals." } } },
                Some(all) => {
                    let mine: Vec<Value> = all.into_iter().filter(|g| g["horizon"] == horizon && g["status"] != "dropped").collect();
                    rsx! {
                        for sec in SECTIONS {
                            {
                                let list: Vec<Value> = mine.iter().filter(|g| g["section"] == sec).cloned().collect();
                                let show = !list.is_empty() || sec != "other";
                                rsx! {
                                    if show {
                                        div { key: "{sec}", class: "mc-section", "data-hue": "{hue_of(sec)}",
                                            h3 { class: "mc-section-h", span { class: "mc-dot" } "{section_name(sec)}" }
                                            div { class: "mc-deck",
                                                for g in list.iter() {
                                                    GoalCard { key: "{txt(g, \"id\")}", g: g.clone() }
                                                }
                                                AddGoalCard { section: sec, horizon }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn StatementCard(s: Value) -> Element {
    let mut mc = use_context::<Mc>();
    let votes = s["votes_14d"].as_i64().unwrap_or(0);
    let id = txt(&s, "id");
    let id2 = id.clone();
    let ev = s["last_evidence"].as_array().cloned().unwrap_or_default();
    rsx! {
        article { class: "mc-card mc-statement", "data-hue": "{hue_of(s[\"section\"].as_str().unwrap_or(\"\"))}",
            span { class: "mc-card-label", "{section_name(s[\"section\"].as_str().unwrap_or(\"\"))}" }
            textarea { class: "mc-big-text", rows: "3", value: "{txt(&s, \"text\")}",
                onchange: move |e| {
                    let v = e.value();
                    if !v.trim().is_empty() { mc.write("mission_identity_update", json!({ "id": id.clone(), "text": v.trim() })); }
                }
            }
            div { class: "mc-votes", aria_label: "{votes} of 14 days",
                for d in 0..14 { span { key: "{d}", class: if (d as i64) < votes { "on" } } }
            }
            span { class: "mc-meta", "{votes} of 14 days" if s["soft_14d"].as_i64().unwrap_or(0) > 0 { " · {s[\"soft_14d\"]} said, {s[\"hard_14d\"]} done" } }
            for e in ev.iter().take(2) {
                span { class: "mc-ev", "✓ {txt(e, \"label\")} · {short_day(e[\"at\"].as_str().unwrap_or(\"\"))}" }
            }
            button { class: "mc-ghost", r#type: "button",
                onclick: move |_| {
                    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
                    if let Some(st) = mc.statements.write().iter_mut().find(|x| x["id"] == id2.as_str()) {
                        st["votes_14d"] = json!(st["votes_14d"].as_i64().unwrap_or(0) + 1);
                    }
                    mc.write("mission_evidence_add", json!({ "from_kind": "manual", "from_id": today, "to_kind": "statement", "to_id": id2.clone(), "relation": "supports", "note": "Voted in the app" }));
                },
                Icon { name: IconName::Check, size: 16 } "I acted on this today"
            }
        }
    }
}

#[component]
fn GoalCard(g: Value) -> Element {
    let mut mc = use_context::<Mc>();
    let id = txt(&g, "id");
    let p = g["progress"].as_f64().unwrap_or(0.0);
    let pct = (p * 100.0).round() as i64;
    let votes = g["votes_14d"].as_i64().unwrap_or(0);
    let steps = g["children"].as_array().cloned().unwrap_or_default();
    let ev = g["last_evidence"].as_array().cloned().unwrap_or_default();
    let mut new_step = use_signal(String::new);
    let gid = id.clone();
    let gid2 = id.clone();
    let done = g["status"] == "done";
    rsx! {
        article { class: "mc-card mc-goal", class: if done { "is-done" }, "data-hue": "{hue_of(g[\"section\"].as_str().unwrap_or(\"\"))}",
            div { class: "mc-goal-top",
                span { class: "mc-card-label", if done { "Reached" } else { "{txt(&g, \"horizon\")} term" } }
                span { class: "mc-pct", "{pct}%" }
            }
            input { class: "mc-goal-title", r#type: "text", value: "{txt(&g, \"title\")}",
                onchange: move |e| { let v = e.value(); if !v.trim().is_empty() { mc.write("mission_goal_update", json!({ "id": gid.clone(), "title": v.trim() })); } }
            }
            if let Some(w) = g["why"].as_str() { p { class: "mc-why", "{w}" } }
            div { class: "mc-bar-track", div { class: "mc-bar-fill", style: "transform: scaleX({p});" } }
            span { class: "mc-meta",
                "{g[\"steps_done\"]}/{g[\"steps_total\"]} steps · {votes} of 14 days moved"
                if let (Some(c), Some(t)) = (g["current"].as_f64(), g["target"].as_f64()) { " · {c} / {t} {txt(&g, \"unit\")}" }
            }
            ul { class: "mc-steps",
                for s in steps.iter() {
                    StepRow { key: "{txt(s, \"id\")}", s: s.clone(), depth: 0 }
                }
            }
            form { class: "mc-step-add",
                onsubmit: move |e| {
                    e.prevent_default();
                    let t = new_step.peek().trim().to_string();
                    if t.is_empty() { return; }
                    let sid = mc.new_id();
                    add_child_local(mc, &gid2, json!({ "id": sid, "title": t, "status": "open", "children": [] }));
                    mc.write("mission_goal_add", json!({ "id": sid, "title": t, "parent_id": gid2.clone() }));
                    new_step.set(String::new());
                },
                input { r#type: "text", placeholder: "Add a step", value: "{new_step}", oninput: move |e| new_step.set(e.value()) }
            }
            for e in ev.iter().take(3) {
                span { class: "mc-ev", "✓ {txt(e, \"label\")} · {short_day(e[\"at\"].as_str().unwrap_or(\"\"))}" }
            }
        }
    }
}

/// Insert a new step under `parent` in the local tree.
fn add_child_local(mut mc: Mc, parent: &str, child: Value) {
    fn walk(list: &mut [Value], parent: &str, child: &Value) -> bool {
        for g in list.iter_mut() {
            if g["id"] == parent {
                if let Some(c) = g["children"].as_array_mut() { c.push(child.clone()); } else { g["children"] = json!([child]); }
                return true;
            }
            if let Some(c) = g["children"].as_array_mut() {
                if walk(c, parent, child) {
                    return true;
                }
            }
        }
        false
    }
    if let Some(list) = mc.goals.write().as_mut() {
        walk(list, parent, &child);
    }
}

fn set_status_local(mut mc: Mc, id: &str, status: &str) {
    fn walk(list: &mut [Value], id: &str, status: &str) {
        for g in list.iter_mut() {
            if g["id"] == id {
                g["status"] = json!(status);
            }
            if let Some(c) = g["children"].as_array_mut() {
                walk(c, id, status);
            }
        }
    }
    if let Some(list) = mc.goals.write().as_mut() {
        walk(list, id, status);
    }
}

#[component]
fn StepRow(s: Value, depth: u8) -> Element {
    let mut mc = use_context::<Mc>();
    let id = txt(&s, "id");
    let done = s["status"] == "done";
    let subs = s["children"].as_array().cloned().unwrap_or_default();
    let mut adding = use_signal(|| false);
    let mut sub = use_signal(String::new);
    let (id1, id2, id3, id4) = (id.clone(), id.clone(), id.clone(), id.clone());
    rsx! {
        li { class: "mc-step", class: if done { "is-done" }, "data-depth": "{depth}",
            div { class: "mc-step-row",
                button { class: "mc-check", r#type: "button", aria_label: if done { "Mark open" } else { "Mark done" },
                    onclick: move |_| {
                        let next = if done { "open" } else { "done" };
                        set_status_local(mc, &id1, next);
                        mc.write("mission_goal_update", json!({ "id": id1.clone(), "status": next }));
                    },
                    if done { Icon { name: IconName::Check, size: 13, stroke: 2.6 } }
                }
                input { class: "mc-step-title", r#type: "text", value: "{txt(&s, \"title\")}",
                    onchange: move |e| { let v = e.value(); if !v.trim().is_empty() { mc.write("mission_goal_update", json!({ "id": id2.clone(), "title": v.trim() })); } }
                }
                if depth == 0 {
                    button { class: "mc-mini", r#type: "button", aria_label: "Add substep", onclick: move |_| adding.set(!adding()), Icon { name: IconName::Plus, size: 13 } }
                }
                button { class: "mc-mini", r#type: "button", aria_label: "Remove step",
                    onclick: move |_| {
                        set_status_local(mc, &id3, "dropped");
                        mc.write("mission_goal_update", json!({ "id": id3.clone(), "deleted": true }));
                    },
                    Icon { name: IconName::X, size: 12 }
                }
            }
            if !subs.is_empty() {
                ul { class: "mc-steps mc-substeps",
                    for x in subs.iter().filter(|x| x["status"] != "dropped") {
                        StepRow { key: "{txt(x, \"id\")}", s: x.clone(), depth: depth + 1 }
                    }
                }
            }
            if adding() {
                form { class: "mc-step-add mc-sub-add",
                    onsubmit: move |e| {
                        e.prevent_default();
                        let t = sub.peek().trim().to_string();
                        if t.is_empty() { return; }
                        let sid = mc.new_id();
                        add_child_local(mc, &id4, json!({ "id": sid, "title": t, "status": "open", "children": [] }));
                        mc.write("mission_goal_add", json!({ "id": sid, "title": t, "parent_id": id4.clone() }));
                        sub.set(String::new());
                        adding.set(false);
                    },
                    input { r#type: "text", placeholder: "Substep", value: "{sub}", autofocus: true, oninput: move |e| sub.set(e.value()) }
                }
            }
        }
    }
}

#[component]
fn AddGoalCard(section: &'static str, horizon: &'static str) -> Element {
    let mut mc = use_context::<Mc>();
    let mut title = use_signal(String::new);
    rsx! {
        form { class: "mc-card mc-card-add", "data-hue": "{hue_of(section)}",
            onsubmit: move |e| {
                e.prevent_default();
                let t = title.peek().trim().to_string();
                if t.is_empty() { return; }
                let id = mc.new_id();
                if let Some(list) = mc.goals.write().as_mut() {
                    list.push(json!({ "id": id, "title": t, "section": section, "horizon": horizon, "status": "open", "progress": 0.0, "votes_14d": 0, "steps_done": 0, "steps_total": 0, "children": [] }));
                }
                mc.write("mission_goal_add", json!({ "id": id, "title": t, "section": section, "horizon": horizon }));
                title.set(String::new());
            },
            span { class: "mc-card-label", "New {section_name(section).to_lowercase()} goal" }
            input { r#type: "text", placeholder: "What you'll reach", value: "{title}", oninput: move |e| title.set(e.value()) }
            button { class: "btn", r#type: "submit", disabled: title().trim().is_empty(), "Add" }
        }
    }
}
