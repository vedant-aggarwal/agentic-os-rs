//! Under the hero: the stats for whatever stands in front, mood and energy, shortcuts.
//! On desktop, also Nora's card in the right panel.

use dioxus::prelude::*;

use crate::components::{Icon, IconName};
use crate::data::goal::Goal;
use crate::data::mood::MOOD_HUES;
use crate::live::{Live, TodayView};
use crate::state::{AppState, SheetKind};

use super::topbar::NORA_ROUND;

struct Stat {
    label: &'static str,
    value: String,
    /// "of 3" after the value, muted.
    of: Option<String>,
    /// Bar fill, 0..=1, drawn in the pillar's colour range.
    pct: Option<f32>,
}

fn ratio(label: &'static str, done: usize, total: usize) -> Stat {
    Stat {
        label,
        value: done.to_string(),
        of: Some(format!("of {total}")),
        pct: Some(if total == 0 { 0.0 } else { done as f32 / total as f32 }),
    }
}

fn plain(label: &'static str, value: String) -> Stat {
    Stat { label, value, of: None, pct: None }
}

/// (done, total) of today's top-level tasks, optionally only for one pillar.
fn task_count(t: &TodayView, pillar: Option<&str>) -> (usize, usize) {
    let tasks: Vec<_> = t
        .areas
        .iter()
        .filter(|g| pillar.is_none() || g.pillar_id.as_deref() == pillar)
        .flat_map(|g| g.tasks.iter().map(|n| &n.task))
        .filter(|t| t.status != "dropped")
        .collect();
    (tasks.iter().filter(|t| t.done()).count(), tasks.len())
}

fn habit_count(t: &TodayView, areas: Option<&[String]>, hidden: &[String]) -> (usize, usize) {
    let hs: Vec<_> = t
        .habits
        .iter()
        .filter(|h| h.due_today && !hidden.contains(&h.id))
        .filter(|h| areas.is_none_or(|a| h.area_id.as_ref().is_some_and(|x| a.contains(x))))
        .collect();
    (hs.iter().filter(|h| h.counts()).count(), hs.len())
}

#[component]
pub fn Stats() -> Element {
    let state = use_context::<AppState>();
    let live = use_context::<Live>();
    let safe = (state.safe)();
    let t = live.today.read().clone().unwrap_or_default();
    let tree = live.areas.read().clone();
    let hidden = if safe { crate::live::PROFILE.read().private_habits.clone() } else { Vec::new() };
    let (td, tt) = task_count(&t, None);
    let (hd, ht) = habit_count(&t, None, &hidden);
    let me = [ratio("Tasks", td, tt), ratio("Habits", hd, ht), plain("XP today", format!("+{}", t.xp.today.max(0)))];
    let goal_stats = |g: Goal| -> [Stat; 3] {
        let areas: Vec<String> = tree.pillars.iter().filter(|p| p.id == g.pillar_id()).flat_map(|p| p.areas.iter().map(|a| a.id.clone())).collect();
        let (d, n) = task_count(&t, Some(g.pillar_id()));
        let (hd, ht) = habit_count(&t, Some(&areas), &hidden);
        [plain(g.stat(), format!("Lv {}", g.stat_level())), ratio("Tasks", d, n), ratio("Habits", hd, ht)]
    };

    let mut rows: Vec<(&'static str, [Stat; 3])> = vec![
        ("me", me),
        ("physical", goal_stats(Goal::Physical)),
        ("money", goal_stats(Goal::Money)),
    ];
    if !safe {
        rows.push(("mind", goal_stats(Goal::Mind)));
    }

    rsx! {
        div { class: "stats", aria_live: "polite",
            for (kind, cells) in rows {
                div { key: "{kind}", class: "stats-row stats-{kind}", "data-kind": kind,
                    for cell in cells {
                        div { key: "{cell.label}", class: "stat",
                            span { class: "stat-label", "{cell.label}" }
                            span { class: "stat-value",
                                "{cell.value}"
                                if let Some(of) = cell.of {
                                    span { class: "stat-of", " {of}" }
                                }
                            }
                            if let Some(p) = cell.pct {
                                span { class: "stat-bar", style: "--p: {p:.3}" }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
pub fn MoodControl() -> Element {
    let mut state = use_context::<AppState>();
    let live = use_context::<Live>();
    let checkin = (state.checkin)();
    // The latest check-in today from the server when this session has none.
    let last = live.today.read().as_ref().and_then(|t| t.moods.last().cloned());
    let (title, sub, hue, due) = match (&checkin, last) {
        (Some(c), _) => (c.summary(), format!("Checked in at {}. Tap to update", c.at), MOOD_HUES[c.mood as usize], false),
        (None, Some(m)) => {
            let idx = (((m.valence + 1.0) * 3.0).round() as i64).clamp(0, 6) as usize;
            let e = (m.energy.unwrap_or(3) - 1).clamp(0, 4) as usize;
            let at = chrono::DateTime::parse_from_rfc3339(&m.at)
                .map(|d| d.with_timezone(&chrono::Local).format("%H:%M").to_string())
                .unwrap_or_default();
            (
                format!("{}, energy {}", crate::data::mood::MOODS[idx], crate::data::mood::ENERGY[e].to_lowercase()),
                format!("Checked in at {at}. Tap to update"),
                MOOD_HUES[idx],
                false,
            )
        }
        _ => ("How are you right now?".to_string(), "Mood and energy, ten seconds".to_string(), "#9a9aa6", true),
    };

    rsx! {
        button {
            class: "mood-ctl",
            class: if due { "is-due" },
            r#type: "button",
            style: "--mood: {hue}",
            onclick: move |_| state.open_sheet(SheetKind::Mood),
            span { class: "mood-orb", aria_hidden: "true" }
            span { class: "mood-text",
                span { class: "mood-title", "{title}" }
                span { class: "mood-sub", "{sub}" }
            }
            Icon { name: IconName::ChevronRight, size: 18, stroke: 1.75 }
        }
    }
}

const SHORTCUTS: [(SheetKind, &str, IconName); 5] = [
    (SheetKind::Call, "Call Nora", IconName::Phone),
    (SheetKind::Wave, "Wave", IconName::Waves),
    (SheetKind::Capture, "Capture", IconName::Mic),
    (SheetKind::Print, "Print plan", IconName::Printer),
    (SheetKind::Launch, "Launch work", IconName::Rocket),
];

#[component]
pub fn Shortcuts() -> Element {
    let mut state = use_context::<AppState>();
    rsx! {
        nav { class: "shortcuts", aria_label: "Shortcuts",
            for (kind, label, icon) in SHORTCUTS.into_iter().filter(|(k, _, _)| !(*k == SheetKind::Wave && (state.safe)())) {
                button {
                    key: "{label}",
                    class: "sc sc-{kind.slug()}",
                    r#type: "button",
                    onclick: move |_| state.open_sheet(kind),
                    span { class: "sc-ico", Icon { name: icon, size: 21, stroke: 1.75 } }
                    span { class: "sc-label", "{label}" }
                }
            }
        }
    }
}

/// Desktop right panel: Nora first, with her latest words.
#[component]
pub fn NoraCard() -> Element {
    let mut state = use_context::<AppState>();
    let live = use_context::<Live>();
    let lines: Vec<String> = live
        .nora
        .read()
        .iter()
        .rev()
        .filter(|m| m.engine != "user")
        .take(3)
        .map(|m| m.text.clone())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let typing = live.nora_live.read().is_some();
    rsx! {
        section { class: "nora-card",
            div { class: "nc-head",
                button {
                    class: "nc-face",
                    r#type: "button",
                    aria_label: "Open Nora",
                    onclick: move |_| state.nora.set(true),
                    span { class: "nb-ring", aria_hidden: "true" }
                    img { src: NORA_ROUND, alt: "" }
                }
                div { class: "nc-who",
                    span { class: "nc-name", "Nora" }
                    span { class: "nc-status", if typing { "Writing…" } else if live.paired() { "Free to talk" } else { "Not connected" } }
                }
                button {
                    class: "icon-btn nc-call",
                    r#type: "button",
                    aria_label: "Call Nora",
                    onclick: move |_| state.open_sheet(SheetKind::Call),
                    Icon { name: IconName::Phone, size: 18, stroke: 1.75 }
                }
            }
            ul { class: "nc-brief",
                if lines.is_empty() {
                    li { "Nothing from her yet today. Ask her anything." }
                }
                for (i, line) in lines.iter().enumerate() {
                    li { key: "{i}", "{line}" }
                }
            }
            button {
                class: "nc-ask",
                r#type: "button",
                onclick: move |_| state.nora.set(true),
                span { "Ask Nora" }
                Icon { name: IconName::Mic, size: 18, stroke: 1.75 }
            }
        }
    }
}
