//! New Me: swipeable, editable cards (rules, understanding, read-when-in-doubt, tricks) and the
//! data drop. Rules with a check get kept / broken for today; the rate is over the last 14 checks.

use dioxus::prelude::*;
use serde_json::{Value, json};

use super::data::{Mc, short_day, txt};
use crate::components::{Icon, IconName};
use crate::live::Live;

const KINDS: [(&str, &str, &str, &str); 4] = [
    ("usool", "Rules", "Usool: what you do, whatever the mood.", "me"),
    ("understanding", "Understanding", "What you've learned about how you work.", "mind"),
    ("in_doubt", "Read when in doubt", "Lines for the hard moments.", "money"),
    ("trick", "Tricks", "Small moves that work on you.", "physical"),
];

#[component]
pub fn NewMeView() -> Element {
    let mut mc = use_context::<Mc>();
    let live = use_context::<Live>();
    use_effect(move || {
        if live.conn.read().is_some() {
            spawn(async move { mc.load_newme().await });
        }
    });
    let rules = (mc.rules)();
    let drops = (mc.drops)();
    let mut drop_text = use_signal(String::new);
    let mut drop_kind = use_signal(|| "observation".to_string());

    rsx! {
        section { class: "mc-newme",
            match rules {
                None => rsx! { p { class: "mc-empty", if live.paired() { "Loading…" } else { "Pair this device to see New Me." } } },
                Some(all) => rsx! {
                    for (kind, title, lead, hue) in KINDS {
                        {
                            let list: Vec<Value> = all.iter().filter(|r| r["kind"] == kind).cloned().collect();
                            rsx! {
                                div { key: "{kind}", class: "mc-section", "data-hue": "{hue}",
                                    h3 { class: "mc-section-h", span { class: "mc-dot" } "{title}" }
                                    p { class: "mc-sub", "{lead}" }
                                    div { class: "mc-deck",
                                        for r in list.iter() {
                                            RuleCard { key: "{txt(r, \"id\")}", r: r.clone() }
                                        }
                                        AddRuleCard { kind }
                                    }
                                }
                            }
                        }
                    }
                },
            }
            div { class: "mc-section mc-drop", "data-hue": "mind",
                h3 { class: "mc-section-h", span { class: "mc-dot" } "Data drop" }
                p { class: "mc-sub", "Things you notice about yourself. Three alike become a pattern; a confirmed one can become a rule." }
                form { class: "mc-drop-add",
                    onsubmit: move |e| {
                        e.prevent_default();
                        let t = drop_text.peek().trim().to_string();
                        if t.is_empty() { return; }
                        let id = mc.new_id();
                        let k = drop_kind.peek().clone();
                        mc.drops.write().insert(0, json!({ "id": id, "text": t, "kind": k, "status": "open", "by": "owner", "at": chrono::Utc::now().to_rfc3339() }));
                        mc.write("mission_drop_add", json!({ "id": id, "text": t, "kind": k }));
                        drop_text.set(String::new());
                    },
                    textarea { rows: "2", placeholder: "What did you notice today?", value: "{drop_text}", oninput: move |e| drop_text.set(e.value()) }
                    div { class: "mc-drop-row",
                        select { value: "{drop_kind}", onchange: move |e| drop_kind.set(e.value()),
                            for k in ["observation", "pattern", "learning", "experiment", "metric"] { option { key: "{k}", value: "{k}", "{k}" } }
                        }
                        button { class: "btn btn-primary", r#type: "submit", disabled: drop_text().trim().is_empty(), "Drop it" }
                    }
                }
                div { class: "ph-list mc-drop-list",
                    if drops.is_empty() {
                        div { class: "ph-row ph-muted", span { class: "ph-text", "Nothing yet. Nora adds what she notices after your 21:00 talk, marked as hers." } }
                    }
                    for d in drops.iter() {
                        DropRow { key: "{txt(d, \"id\")}", d: d.clone() }
                    }
                }
            }
        }
    }
}

fn patch_rule(mut mc: Mc, id: &str, f: impl FnOnce(&mut Value)) {
    let mut guard = mc.rules.write();
    if let Some(r) = guard.as_mut().and_then(|list| list.iter_mut().find(|r| r["id"] == id)) {
        f(r);
    }
}

#[component]
fn RuleCard(r: Value) -> Element {
    let mut mc = use_context::<Mc>();
    let id = txt(&r, "id");
    let checks = r["checks"].as_i64().unwrap_or(0);
    let kept = r["kept"].as_i64().unwrap_or(0);
    let rate = r["kept_rate"].as_f64();
    let is_rule = r["kind"] == "usool";
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let checked_today = r["last_check"].as_str() == Some(today.as_str());
    let by_nora = r["origin"] == "nora";
    let (i1, i2, i3, i4, i5, i6) = (id.clone(), id.clone(), id.clone(), id.clone(), id.clone(), id.clone());
    let mut save = move |field: &'static str, id: String, v: String| {
        let val = if v.trim().is_empty() { Value::Null } else { json!(v.trim()) };
        patch_rule(mc, &id, |x| x[field] = val.clone());
        mc.write("mission_rule_update", json!({ "id": id, field: val }));
    };
    let mut check = move |id: String, kept_now: bool| {
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        patch_rule(mc, &id, |x| {
            if x["last_check"].as_str() != Some(today.as_str()) {
                x["checks"] = json!(x["checks"].as_i64().unwrap_or(0) + 1);
                x["kept"] = json!(x["kept"].as_i64().unwrap_or(0) + kept_now as i64);
            }
            x["last_check"] = json!(today);
            x["today_kept"] = json!(kept_now);
        });
        mc.write("mission_rule_check", json!({ "id": id, "kept": kept_now }));
    };
    rsx! {
        article { class: "mc-card mc-rule", class: if r["unchecked"] == true { "needs-you" },
            div { class: "mc-goal-top",
                span { class: "mc-card-label", if by_nora { "Nora's draft" } else if is_rule { "Rule" } else { "Card" } }
                if let Some(v) = r["verified_at"].as_str() { span { class: "mc-meta", "true as of {short_day(v)}" } }
            }
            textarea { class: "mc-big-text", rows: "3", value: "{txt(&r, \"text\")}", onchange: move |e| save("text", i1.clone(), e.value()) }
            if is_rule {
                div { class: "mc-ifthen",
                    label { span { "When" } input { r#type: "text", value: "{txt(&r, \"trigger_when\")}", placeholder: "the trigger", onchange: move |e| save("trigger_when", i2.clone(), e.value()) } }
                    label { span { "Then" } input { r#type: "text", value: "{txt(&r, \"action_then\")}", placeholder: "what you do", onchange: move |e| save("action_then", i3.clone(), e.value()) } }
                }
                div { class: "mc-bar-track", div { class: "mc-bar-fill", style: "transform: scaleX({rate.unwrap_or(0.0)});" } }
                span { class: "mc-meta",
                    if checks == 0 { "Not checked yet" } else { "Kept {kept} of the last {checks}" }
                    if r["unchecked"] == true { " · unchecked for 14 days" }
                }
                div { class: "mc-kept",
                    button { r#type: "button", class: if checked_today && r["today_kept"] != false { "on" }, onclick: move |_| check(i4.clone(), true), Icon { name: IconName::Check, size: 15 } "Kept today" }
                    button { r#type: "button", class: if checked_today && r["today_kept"] == false { "on off" }, onclick: move |_| check(i5.clone(), false), "Broke it" }
                }
            }
            div { class: "mc-card-foot",
                button { class: "mc-mini-text", r#type: "button",
                    onclick: { let id = id.clone(); move |_| {
                        patch_rule(mc, &id, |x| x["verified_at"] = json!(chrono::Local::now().format("%Y-%m-%d").to_string()));
                        mc.write("mission_node_verify", json!({ "kind": "rule", "id": id.clone() }));
                    } },
                    "Still true"
                }
                button { class: "mc-mini-text", r#type: "button",
                    onclick: move |_| {
                        if let Some(list) = mc.rules.write().as_mut() { list.retain(|x| x["id"] != i6.as_str()); }
                        mc.write("mission_rule_update", json!({ "id": i6.clone(), "status": "retired" }));
                    },
                    "Retire"
                }
            }
        }
    }
}

#[component]
fn AddRuleCard(kind: &'static str) -> Element {
    let mut mc = use_context::<Mc>();
    let mut text = use_signal(String::new);
    rsx! {
        form { class: "mc-card mc-card-add",
            onsubmit: move |e| {
                e.prevent_default();
                let t = text.peek().trim().to_string();
                if t.is_empty() { return; }
                let id = mc.new_id();
                if let Some(list) = mc.rules.write().as_mut() {
                    list.push(json!({ "id": id, "text": t, "kind": kind, "status": "active", "checks": 0, "kept": 0, "check_mode": if kind == "usool" { "nightly" } else { "none" } }));
                }
                mc.write("mission_rule_add", json!({ "id": id, "text": t, "kind": kind }));
                text.set(String::new());
            },
            span { class: "mc-card-label", "New card" }
            textarea { rows: "3", placeholder: if kind == "usool" { "Never… / Always…" } else { "Write it the way you'd say it" }, value: "{text}", oninput: move |e| text.set(e.value()) }
            button { class: "btn", r#type: "submit", disabled: text().trim().is_empty(), "Add" }
        }
    }
}

#[component]
fn DropRow(d: Value) -> Element {
    let mut mc = use_context::<Mc>();
    let id = txt(&d, "id");
    let status = txt(&d, "status");
    let by_nora = d["by"] == "nora";
    let mut set = move |id: String, status: &'static str| {
        if let Some(x) = mc.drops.write().iter_mut().find(|x| x["id"] == id.as_str()) {
            x["status"] = json!(status);
        }
        mc.write("mission_drop_update", json!({ "id": id, "status": status }));
    };
    let (i1, i2, i3) = (id.clone(), id.clone(), id.clone());
    let text = txt(&d, "text");
    rsx! {
        div { class: "ph-row mc-drop-item", "data-status": "{status}",
            span { class: "ph-kind", "{txt(&d, \"kind\")}" }
            div { class: "mc-drop-body",
                span { class: "ph-text", "{text}" }
                span { class: "mc-meta",
                    if by_nora { "Nora's read · " }
                    "{short_day(d[\"at\"].as_str().unwrap_or(\"\"))} · {status.replace('_', \" \")}"
                }
                if status == "open" {
                    div { class: "mc-drop-actions",
                        button { r#type: "button", onclick: move |_| set(i1.clone(), "confirmed"), "Confirm" }
                        button { r#type: "button", onclick: move |_| set(i2.clone(), "rejected"), "Not true" }
                        button { r#type: "button",
                            onclick: move |_| {
                                let rid = mc.new_id();
                                if let Some(list) = mc.rules.write().as_mut() {
                                    list.push(json!({ "id": rid, "text": text.clone(), "kind": "usool", "status": "active", "checks": 0, "kept": 0, "check_mode": "nightly" }));
                                }
                                if let Some(x) = mc.drops.write().iter_mut().find(|x| x["id"] == i3.as_str()) { x["status"] = json!("became_rule"); }
                                mc.write("mission_rule_add", json!({ "id": rid, "text": text.clone(), "origin_drop_id": i3.clone() }));
                            },
                            "Make it a rule"
                        }
                    }
                }
            }
        }
    }
}
