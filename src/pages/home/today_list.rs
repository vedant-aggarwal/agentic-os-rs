//! Today below the hero (SYSTEM v2): tasks by pillar and area with subtasks, habits with
//! streaks (never miss twice), and a compact timeline. Everything comes from the server's
//! `today` tool; writes go local first through the outbox (`crate::live`).
//! On desktop this sits in the right panel.

use dioxus::prelude::*;

use crate::components::{Icon, IconName};
use crate::data::goal::Goal;
use crate::live::{self, AreaTasks, HabitView, Live, Task, TaskNode, TodayView};
use crate::state::{AppState, SheetKind};

const SKIP_REASONS: [&str; 7] = ["Tired", "Forgot", "Too big", "Avoided", "Phone, reels", "Sick", "Other"];
const LONG_PRESS_MS: u128 = 450;

/// Hue slug for a pillar id, so rows take the pillar's colour range.
fn hue(pillar: Option<&str>) -> &'static str {
    pillar.and_then(Goal::from_pillar_id).map_or("me", Goal::slug)
}

#[component]
pub fn TodayCard() -> Element {
    let live = use_context::<Live>();
    let mut state = use_context::<AppState>();
    let today = live.today.read().clone();
    let paired = live.paired();
    let offline = paired && !(live.online)();
    let pending = live.outbox.read().len();

    rsx! {
        section { class: "today-card live-today", aria_label: "Today",
            if !paired && (live.ready)() {
                button {
                    class: "lt-banner",
                    r#type: "button",
                    onclick: move |_| state.open_sheet(SheetKind::Profile),
                    Icon { name: IconName::Shield, size: 16, stroke: 2.0 }
                    span { "Not connected to your server. Tap to pair this device." }
                }
            } else if offline || (live.auth_bad)() {
                p { class: "lt-banner lt-quiet",
                    if (live.auth_bad)() { "The server refused this device. Re-pair in You." } else { "Offline. Changes are saved here and sent when you're back." }
                    if pending > 0 { " {pending} waiting." }
                }
            }
            match today {
                None => rsx! {
                    p { class: "lt-empty", if paired { "Loading today…" } else { "Your day shows here once this device is paired." } }
                },
                Some(t) => rsx! {
                    Timeline { today: t.clone() }
                    Tasks { today: t.clone() }
                    Habits { today: t }
                },
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Tasks

#[component]
fn Tasks(today: TodayView) -> Element {
    let live = use_context::<Live>();
    let tree = live.areas.read().clone();
    // Order groups pillar by pillar, the way the areas are sorted on the server.
    let mut groups: Vec<AreaTasks> = today.areas.iter().filter(|g| !g.tasks.is_empty()).cloned().collect();
    let rank = |g: &AreaTasks| {
        let mut i = 0usize;
        for p in &tree.pillars {
            for a in &p.areas {
                if Some(&a.id) == g.area_id.as_ref() {
                    return i;
                }
                i += 1;
            }
        }
        usize::MAX
    };
    groups.sort_by_key(rank);
    let all: Vec<&Task> = groups.iter().flat_map(|g| g.tasks.iter().map(|n| &n.task)).collect();
    let done = all.iter().filter(|t| t.done()).count();
    let total = all.iter().filter(|t| t.status != "dropped").count();

    rsx! {
        div { class: "tc-head",
            h2 { class: "tc-h", "Tasks" }
            span { class: "tc-count", "{done} of {total}" }
        }
        if groups.is_empty() {
            p { class: "lt-empty", "No tasks for today. Add the first one below, or tell Nora \"add a task: …\"." }
        }
        for g in groups {
            div { key: "{g.area_id.clone().unwrap_or_default()}", class: "lt-area", "data-hue": hue(g.pillar_id.as_deref()),
                p { class: "lt-area-name", "{g.area.clone().unwrap_or_else(|| \"No area yet\".into())}" }
                ul { class: "tc-list",
                    for n in g.tasks.iter().cloned() {
                        TaskRow { key: "{n.task.id}", node: n, hue: hue(g.pillar_id.as_deref()) }
                    }
                }
            }
        }
        AddTask {}
    }
}

#[component]
fn TaskRow(node: TaskNode, hue: &'static str) -> Element {
    let mut live = use_context::<Live>();
    let mut float = use_signal(|| (0u32, 0i64));
    let mut pressed = use_signal(|| None::<web_time::Instant>);
    let mut skipping = use_signal(|| false);
    let mut adding = use_signal(|| false);
    let mut sub_draft = use_signal(String::new);
    let t = node.task.clone();
    let (seq, amount) = float();
    let id = t.id.clone();
    let xp = t.xp();
    let closed = t.closed();
    let local = t.local();
    let subs_done = node.subtasks.iter().filter(|s| s.done()).count();

    let id_tick = id.clone();
    let id_skip = id.clone();
    let id_sub = id.clone();
    rsx! {
        li { class: "task", class: if t.done() { "done" }, class: if t.status == "skipped" { "skipped" }, "data-hue": hue,
            button {
                class: "task-hit",
                r#type: "button",
                aria_pressed: if t.done() { "true" } else { "false" },
                title: "Tap to tick. Hold to skip with a reason.",
                onpointerdown: move |_| pressed.set(Some(web_time::Instant::now())),
                onpointerleave: move |_| pressed.set(None),
                oncontextmenu: move |e| e.prevent_default(),
                onclick: move |_| {
                    let start = *pressed.peek();
                    let held = start.map(|p| p.elapsed().as_millis()).unwrap_or(0);
                    pressed.set(None);
                    if closed || local {
                        return;
                    }
                    if held >= LONG_PRESS_MS {
                        skipping.set(true);
                        return;
                    }
                    live.tick_task(&id_tick);
                    float.set((seq + 1, xp));
                },
                span { class: "task-check", aria_hidden: "true",
                    Icon { name: if t.status == "skipped" { IconName::X } else { IconName::Check }, size: 14, stroke: 2.75 }
                }
                span { class: "task-text",
                    span { class: "task-title", "{t.title}" }
                    if t.status == "skipped" {
                        span { class: "task-detail", "Skipped: {t.skip_reason.clone().unwrap_or_default()}" }
                    } else if local {
                        span { class: "task-detail", "Saving…" }
                    } else if !node.subtasks.is_empty() {
                        span { class: "task-detail", "{subs_done} of {node.subtasks.len()} steps" }
                    }
                }
                if let Some(tm) = t.time() {
                    span { class: "task-xp", "{tm}" }
                } else {
                    span { class: "task-xp", "{xp} XP" }
                }
            }
            if seq > 0 {
                span { key: "{seq}", class: "xp-float", aria_hidden: "true", "+{amount}" }
            }
            if skipping() {
                div { class: "lt-skip", role: "group", aria_label: "Why skip it?",
                    span { class: "lt-skip-q", "Why skip it? Be honest; no XP is lost." }
                    div { class: "lt-chips",
                        for r in SKIP_REASONS {
                            button {
                                key: "{r}",
                                class: "chip",
                                r#type: "button",
                                onclick: {
                                    let id = id_skip.clone();
                                    move |_| {
                                        live.skip_task(&id, &r.to_lowercase());
                                        skipping.set(false);
                                    }
                                },
                                "{r}"
                            }
                        }
                        button { class: "chip chip-quiet", r#type: "button", onclick: move |_| skipping.set(false), "Cancel" }
                    }
                }
            }
            if !node.subtasks.is_empty() || adding() {
                ul { class: "lt-subs",
                    for s in node.subtasks.iter().cloned() {
                        SubRow { key: "{s.id}", task: s }
                    }
                    if adding() {
                        li { class: "lt-subadd",
                            input {
                                class: "lt-input",
                                placeholder: "Next step",
                                value: "{sub_draft}",
                                oninput: move |e| sub_draft.set(e.value()),
                                onkeydown: {
                                    let id = id_sub.clone();
                                    move |e: KeyboardEvent| {
                                        if e.key() == Key::Enter {
                                            let text = sub_draft.peek().trim().to_string();
                                            if !text.is_empty() {
                                                live.add_task(&text, None, Some(id.clone()));
                                            }
                                            sub_draft.set(String::new());
                                            adding.set(false);
                                        }
                                    }
                                },
                            }
                        }
                    }
                }
            }
            if !closed && !local && !adding() {
                button { class: "lt-substep", r#type: "button", onclick: move |_| adding.set(true), "+ step" }
            }
        }
    }
}

#[component]
fn SubRow(task: Task) -> Element {
    let mut live = use_context::<Live>();
    let id = task.id.clone();
    let can = !task.closed() && !task.local();
    rsx! {
        li { class: "lt-sub", class: if task.done() { "done" },
            button {
                class: "lt-sub-hit",
                r#type: "button",
                onclick: move |_| {
                    if can {
                        live.tick_task(&id);
                    }
                },
                span { class: "lt-sub-dot", aria_hidden: "true",
                    if task.done() { Icon { name: IconName::Check, size: 10, stroke: 3.0 } }
                }
                span { "{task.title}" }
            }
        }
    }
}

#[component]
fn AddTask() -> Element {
    let mut live = use_context::<Live>();
    let mut draft = use_signal(String::new);
    let mut area = use_signal(String::new);
    let tree = live.areas.read().clone();
    let mut add = move || {
        let text = draft.peek().trim().to_string();
        if text.is_empty() {
            return;
        }
        let a = area.peek().clone();
        live.add_task(&text, (!a.is_empty()).then_some(a), None);
        draft.set(String::new());
    };
    rsx! {
        div { class: "lt-add",
            input {
                class: "lt-input",
                placeholder: "Add a task for today",
                aria_label: "Add a task for today",
                value: "{draft}",
                oninput: move |e| draft.set(e.value()),
                onkeydown: move |e| {
                    if e.key() == Key::Enter {
                        add();
                    }
                },
            }
            select {
                class: "lt-select",
                aria_label: "Area",
                value: "{area}",
                onchange: move |e| area.set(e.value()),
                option { value: "", "No area" }
                for p in tree.pillars.iter() {
                    optgroup { key: "{p.id}", label: "{p.name}",
                        for a in p.areas.iter() {
                            option { key: "{a.id}", value: "{a.id}", "{a.name}" }
                        }
                    }
                }
            }
            button { class: "lt-add-btn", r#type: "button", aria_label: "Add task", onclick: move |_| add(),
                Icon { name: IconName::Plus, size: 18, stroke: 2.25 }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Habits and streaks

#[component]
fn Habits(today: TodayView) -> Element {
    let state = use_context::<AppState>();
    let mut live = use_context::<Live>();
    let safe = (state.safe)();
    let private = live::PROFILE.read().private_habits.clone();
    let mut habits: Vec<HabitView> = today.habits.into_iter().filter(|h| !(safe && private.contains(&h.id))).collect();
    habits.sort_by_key(|h| !h.anchor);
    let due = habits.iter().filter(|h| h.due_today).count();
    let done = habits.iter().filter(|h| h.due_today && h.counts()).count();
    let mut adding = use_signal(|| false);
    let mut draft = use_signal(String::new);

    rsx! {
        div { class: "tc-head",
            h2 { class: "tc-h", "Habits" }
            span { class: "tc-count", "{done} of {due}" }
        }
        if habits.is_empty() {
            p { class: "lt-empty", "No habits yet. Start with the anchor: journaling, every night. Add it below." }
        }
        ul { class: "lt-habits",
            for h in habits {
                HabitRow { key: "{h.id}", habit: h }
            }
        }
        if adding() || (live.today.read().as_ref().is_some_and(|t| t.habits.is_empty())) {
            div { class: "lt-add",
                input {
                    class: "lt-input",
                    placeholder: "New habit, e.g. Journal",
                    aria_label: "New habit",
                    value: "{draft}",
                    oninput: move |e| draft.set(e.value()),
                    onkeydown: move |e| {
                        if e.key() == Key::Enter {
                            let text = draft.peek().trim().to_string();
                            if !text.is_empty() {
                                live.add_habit(&text, None);
                            }
                            draft.set(String::new());
                            adding.set(false);
                        }
                    },
                }
            }
        } else {
            button { class: "lt-substep", r#type: "button", onclick: move |_| adding.set(true), "+ habit" }
        }
    }
}

#[component]
fn HabitRow(habit: HabitView) -> Element {
    let mut live = use_context::<Live>();
    let mut float = use_signal(|| 0u32);
    let h = habit.clone();
    let id = h.id.clone();
    let streak = h.streak.current;
    // Never miss twice: one miss bends the streak, the next due day decides.
    let note = match (h.streak.state.as_str(), h.counts()) {
        (_, true) if h.today.as_deref() == Some("floor") => "Floor version. It counts.".to_string(),
        (_, true) => format!("Done. {streak}-day streak"),
        ("bent", false) => "Missed once. Don't miss twice.".to_string(),
        ("intact", false) if streak > 0 => format!("{streak}-day streak. Keep it."),
        _ => h.cue.clone().map(|c| format!("After: {c}")).unwrap_or_else(|| "Start the streak today".into()),
    };
    let next = if h.today.as_deref() == Some("done") { "floor" } else { "done" };
    let local = id.starts_with("local-");
    rsx! {
        li { class: "lt-habit", class: if h.counts() { "done" }, class: if h.anchor { "anchor" }, "data-streak": h.streak.state.clone(),
            button {
                class: "lt-habit-hit",
                r#type: "button",
                aria_pressed: if h.counts() { "true" } else { "false" },
                title: "Tap: done. Tap again: floor version.",
                onclick: move |_| {
                    if local || !h.due_today {
                        return;
                    }
                    if !habit.counts() {
                        float.set(float() + 1);
                    }
                    live.log_habit(&id, next);
                },
                span { class: "lt-habit-ring", aria_hidden: "true",
                    Icon { name: IconName::Check, size: 13, stroke: 3.0 }
                }
                span { class: "task-text",
                    span { class: "task-title",
                        "{h.name}"
                        if h.anchor { span { class: "lt-anchor", "Anchor" } }
                    }
                    span { class: "task-detail", "{note}" }
                }
                span { class: "lt-streak", aria_label: "{streak}-day streak", "{streak}" }
            }
            if float() > 0 {
                span { key: "{float}", class: "xp-float", aria_hidden: "true", "+{habit.xp}" }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Timeline (Structured-style): today's timed tasks and reminders on one line.

#[derive(Clone, PartialEq)]
struct Slot {
    time: String,
    title: String,
    kind: &'static str,
    done: bool,
}

#[component]
fn Timeline(today: TodayView) -> Element {
    let now = chrono::Local::now().format("%H:%M").to_string();
    let date = chrono::Local::now().format("%Y-%m-%d").to_string();
    let mut slots: Vec<Slot> = Vec::new();
    for g in &today.areas {
        for n in &g.tasks {
            for t in std::iter::once(&n.task).chain(n.subtasks.iter()) {
                if let Some(time) = t.time() {
                    slots.push(Slot { time, title: t.title.clone(), kind: "task", done: t.done() });
                }
            }
        }
    }
    for a in &today.alarms {
        if let Ok(at) = chrono::DateTime::parse_from_rfc3339(&a.at) {
            let local = at.with_timezone(&chrono::Local);
            if local.format("%Y-%m-%d").to_string() == date {
                slots.push(Slot { time: local.format("%H:%M").to_string(), title: a.label.clone(), kind: "alarm", done: a.status == "fired" });
            }
        }
    }
    slots.sort_by(|a, b| a.time.cmp(&b.time));
    let now_at = slots.iter().position(|s| s.time > now).unwrap_or(slots.len());

    rsx! {
        div { class: "lt-timeline",
            div { class: "tc-head",
                h2 { class: "tc-h", "Timeline" }
                span { class: "tc-count", "Now {now}" }
            }
            if slots.is_empty() {
                p { class: "lt-empty", "Nothing on the clock today. Tasks with a time and Nora's reminders line up here." }
            }
            ol { class: "lt-line",
                for (i, s) in slots.into_iter().enumerate() {
                    li { key: "{i}-{s.time}", class: "lt-slot lt-{s.kind}", class: if s.done { "done" }, class: if i < now_at { "past" }, class: if i == now_at { "is-next" },
                        span { class: "lt-time", "{s.time}" }
                        span { class: "lt-dot", aria_hidden: "true" }
                        span { class: "lt-title", "{s.title}" }
                    }
                }
            }
        }
    }
}
