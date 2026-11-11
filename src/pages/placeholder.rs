//! Inbox, Life and Mission on real data.
//! - Inbox: `inbox_list` (WhatsApp, Gmail) when the server has it, plus tasks with no area.
//! - Life: the three pillars and their areas; an area shows its tasks, habits and goal.
//! - Mission: the journal, Growth (week XP and streaks), and what comes next.

use dioxus::prelude::*;

use crate::app::Route;
use crate::components::voice::{MicButton, append_text};
use crate::components::{Icon, IconName};
use crate::data::goal::Goal;
use crate::live::{self, Area, InboxState, Live, TaskNode};
use crate::state::AppState;

use super::home::hero::{goal_img, FACE_COMPANION};

// ---------------------------------------------------------------------------------------------
// Inbox

#[component]
pub fn Inbox() -> Element {
    let mut live = use_context::<Live>();
    use_effect(move || {
        if live.conn.read().is_some() {
            spawn(async move { live.load_inbox().await });
        }
    });
    let inbox = live.inbox.read().clone();
    let tasks = live.inbox_tasks.read().clone();
    rsx! {
        Page { title: "Inbox", lead: "Messages, mail and loose tasks in one list.",
            h2 { class: "pg-h", "Messages" }
            div { class: "ph-list",
                match inbox {
                    InboxState::Loading => rsx! {
                        div { class: "ph-row ph-muted", span { class: "ph-text", if live.paired() { "Loading…" } else { "Pair this device with your server to see messages." } } }
                    },
                    InboxState::NoTool => rsx! {
                        div { class: "ph-row ph-muted", span { class: "ph-text", "Your server has no inbox yet. Update it to the build with WhatsApp and Gmail, then open this tab again." } }
                    },
                    InboxState::Failed(e) => rsx! {
                        div { class: "ph-row ph-muted", span { class: "ph-text", "{e}" } }
                    },
                    InboxState::Items(items) if items.is_empty() => rsx! {
                        div { class: "ph-row ph-muted", span { class: "ph-text", "Nothing needs you. New WhatsApp and Gmail messages Nora should read land here." } }
                    },
                    InboxState::Items(items) => rsx! {
                        for it in items {
                            InboxRow { key: "{it.id}", item: it }
                        }
                    },
                }
            }
            h2 { class: "pg-h", "Tasks without an area" }
            div { class: "ph-list",
                if tasks.is_empty() {
                    div { class: "ph-row ph-muted", span { class: "ph-text", "None. Tasks you add without an area, or capture with Nora, wait here." } }
                }
                for n in tasks {
                    div { key: "{n.task.id}", class: "ph-row",
                        span { class: "ph-kind", "Task" }
                        span { class: "ph-text", "{n.task.title}" }
                    }
                }
            }
        }
    }
}

#[component]
fn InboxRow(item: live::InboxItem) -> Element {
    let mut live = use_context::<Live>();
    let who = [&item.sender_name, &item.chat_name].into_iter().find(|s| !s.is_empty()).cloned().unwrap_or_else(|| item.source.clone());
    let body = if item.subject.is_empty() { item.text.clone() } else { format!("{} · {}", item.subject, item.text) };
    let kind = match item.source.as_str() {
        "whatsapp" => "WA",
        "gmail" => "Mail",
        _ => "Msg",
    };
    let id = item.id.clone();
    rsx! {
        div { class: "ph-row ib-row",
            span { class: "ph-kind", "{kind}" }
            span { class: "ib-main",
                span { class: "ib-who", "{who}" }
                span { class: "ib-text", "{body}" }
            }
            button {
                class: "icon-btn ib-done",
                r#type: "button",
                aria_label: "Mark handled",
                onclick: move |_| {
                    live.write("inbox_mark", serde_json::json!({ "id": id }));
                    if let InboxState::Items(list) = &mut *live.inbox.write() {
                        list.retain(|x| x.id != id);
                    }
                },
                Icon { name: IconName::Check, size: 16, stroke: 2.25 }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Life

#[component]
pub fn Life() -> Element {
    let state = use_context::<AppState>();
    let live = use_context::<Live>();
    let safe = (state.safe)();
    let mut area = use_signal(|| None::<Area>);
    let tree = live.areas.read().clone();

    if let Some(a) = area() {
        return rsx! { AreaPage { area: a, on_back: move |_| area.set(None) } };
    }

    rsx! {
        Page { title: "Life", lead: "Three pillars, and the areas under each.",
            if tree.pillars.is_empty() {
                p { class: "lt-empty", if live.paired() { "Loading your pillars…" } else { "Pair this device to see your pillars and areas." } }
            }
            for p in tree.pillars.iter().cloned() {
                {
                    let goal = Goal::from_pillar_id(&p.id);
                    let hidden = safe && goal.is_some_and(Goal::private);
                    let hue = goal.map_or("me", Goal::slug);
                    rsx! {
                        section { key: "{p.id}", class: "life-pillar", "data-hue": hue,
                            div { class: "life-head",
                                if let Some(g) = goal {
                                    img { src: if g == Goal::Mind { if hidden { goal_img(Goal::Money) } else { FACE_COMPANION } } else { goal_img(g) }, alt: "", class: if hidden { "life-img-hidden" } }
                                }
                                span { class: "ph-pillar-text",
                                    span { class: "ph-pillar-name", "{p.name}" }
                                    span { class: "ph-pillar-sub", "Level {live::pillar_level(&p.id)}" }
                                }
                            }
                            div { class: "ph-list",
                                if p.areas.is_empty() {
                                    div { class: "ph-row ph-muted", span { class: "ph-text", "No areas yet. Add them in the seed file on your server." } }
                                }
                                for a in p.areas.iter().cloned() {
                                    button {
                                        key: "{a.id}",
                                        class: "ph-row life-area",
                                        r#type: "button",
                                        onclick: move |_| area.set(Some(a.clone())),
                                        span { class: "ph-text", "{a.name}" }
                                        Icon { name: IconName::ChevronRight, size: 16, stroke: 1.75 }
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
fn AreaPage(area: Area, on_back: EventHandler<()>) -> Element {
    let mut live = use_context::<Live>();
    let mut state = use_context::<AppState>();
    let nav = use_navigator();
    let safe = (state.safe)();
    let id = area.id.clone();
    let tasks = use_resource(move || {
        let id = id.clone();
        async move { live.area_tasks(&id).await }
    });
    let private = live::PROFILE.read().private_habits.clone();
    let habits: Vec<live::HabitView> = live
        .today
        .read()
        .as_ref()
        .map(|t| t.habits.iter().filter(|h| h.area_id.as_deref() == Some(&area.id) && !(safe && private.contains(&h.id))).cloned().collect())
        .unwrap_or_default();
    let goal = area.pillar_id.as_deref().and_then(Goal::from_pillar_id).filter(|g| !(safe && g.private()));
    let task_list: Vec<TaskNode> = tasks.read().clone().unwrap_or_default();
    let hue = goal.map_or("me", Goal::slug);

    rsx! {
        main { class: "page", "data-hue": hue,
            div { class: "amb amb-quiet", aria_hidden: "true" }
            header { class: "page-head",
                button { class: "icon-btn", r#type: "button", aria_label: "Back to Life", onclick: move |_| on_back.call(()),
                    Icon { name: IconName::ChevronLeft, size: 22, stroke: 2.0 }
                }
                h1 { class: "page-title", "{area.name}" }
            }
            h2 { class: "pg-h", "Tasks" }
            div { class: "ph-list",
                if tasks.read().is_none() {
                    div { class: "ph-row ph-muted", span { class: "ph-text", "Loading…" } }
                } else if task_list.is_empty() {
                    div { class: "ph-row ph-muted", span { class: "ph-text", "No open tasks here. Add one on Today and pick this area." } }
                }
                for n in task_list {
                    div { key: "{n.task.id}", class: "ph-row",
                        span { class: "ph-kind", if n.task.done() { "Done" } else { "Open" } }
                        span { class: "ph-text",
                            "{n.task.title}"
                            if !n.subtasks.is_empty() {
                                span { class: "pp-sub", " · {n.subtasks.iter().filter(|s| s.done()).count()} of {n.subtasks.len()} steps" }
                            }
                        }
                    }
                }
            }
            h2 { class: "pg-h", "Habits" }
            div { class: "ph-list",
                if habits.is_empty() {
                    div { class: "ph-row ph-muted", span { class: "ph-text", "No habits in this area yet." } }
                }
                for h in habits {
                    div { key: "{h.id}", class: "ph-row",
                        span { class: "ph-kind", "{h.streak.current}d" }
                        span { class: "ph-text", "{h.name}" }
                    }
                }
            }
            if let Some(g) = goal {
                h2 { class: "pg-h", "Goal" }
                button {
                    class: "pp-goal",
                    r#type: "button",
                    onclick: move |_| {
                        state.tap_goal(g);
                        nav.push(Route::Today { lock: None, mode: None });
                    },
                    img { src: if g == Goal::Mind { FACE_COMPANION } else { goal_img(g) }, alt: "" }
                    span { class: "pp-goal-text",
                        span { class: "pp-sub", "{g.pillar()}" }
                        span { class: "pp-name", "{g.spec().name}" }
                    }
                    Icon { name: IconName::ChevronRight, size: 18, stroke: 1.75 }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Mission

#[component]
pub fn Mission() -> Element {
    let mut live = use_context::<Live>();
    let state = use_context::<AppState>();
    let safe = (state.safe)();
    let mut entry = use_signal(String::new);
    use_effect(move || {
        if live.conn.read().is_some() {
            spawn(async move { live.load_journal().await });
        }
    });
    let journal = live.journal.read().clone();
    let xp = live.xp.read().clone();
    let private = live::PROFILE.read().private_habits.clone();
    let habits: Vec<live::HabitView> = live
        .today
        .read()
        .as_ref()
        .map(|t| t.habits.iter().filter(|h| !(safe && private.contains(&h.id))).cloned().collect())
        .unwrap_or_default();
    let score = live.today.read().as_ref().map(|t| t.score.rate).unwrap_or(0.0);

    rsx! {
        Page { title: "Mission", lead: "Why all of this, and how the week is going.",
            h2 { class: "pg-h", "Growth" }
            div { class: "ms-growth",
                div { class: "stat", span { class: "stat-label", "XP this week" } span { class: "stat-value", "{xp.week}" } }
                div { class: "stat", span { class: "stat-label", "XP today" } span { class: "stat-value", "{xp.today}" } }
                div { class: "stat", span { class: "stat-label", "Today" } span { class: "stat-value", "{(score * 100.0).round()}%" } }
            }
            div { class: "ph-list",
                if habits.is_empty() {
                    div { class: "ph-row ph-muted", span { class: "ph-text", "Streaks show here once you have habits." } }
                }
                for h in habits {
                    div { key: "{h.id}", class: "ph-row",
                        span { class: "ph-kind", "{h.streak.current}d" }
                        span { class: "ph-text",
                            "{h.name}"
                            span { class: "pp-sub", " · best {h.streak.best}" }
                            if h.streak.state == "bent" { span { class: "pp-sub", " · don't miss twice" } }
                        }
                    }
                }
            }
            h2 { class: "pg-h", "Journal" }
            div { class: "ms-compose",
                textarea {
                    rows: "2",
                    placeholder: "Write or say today's entry",
                    aria_label: "Journal entry",
                    value: "{entry}",
                    oninput: move |e| entry.set(e.value()),
                }
                MicButton { target: "journal", on_text: move |t: String| { let v = append_text(&entry.peek(), &t); entry.set(v) } }
                button {
                    class: "btn btn-primary", r#type: "button", disabled: entry().trim().is_empty() || !live.paired(),
                    onclick: move |_| {
                        let t = entry.peek().trim().to_string();
                        if !t.is_empty() {
                            live.add_journal(&t);
                            entry.set(String::new());
                            spawn(async move {
                                crate::live::net::sleep_ms(1500).await;
                                live.load_journal().await;
                            });
                        }
                    },
                    "Add"
                }
            }
            div { class: "ph-list",
                match journal {
                    None => rsx! { div { class: "ph-row ph-muted", span { class: "ph-text", if live.paired() { "Loading…" } else { "Pair this device to read your journal." } } } },
                    Some(list) if list.is_empty() => rsx! {
                        div { class: "ph-row ph-muted", span { class: "ph-text", "No entries yet. Close the day with Nora at 21:00, or tell her \"journal: …\"." } }
                    },
                    Some(list) => rsx! {
                        for e in list {
                            div { key: "{e.id}", class: "ph-row ms-entry",
                                span { class: "ph-kind", "{e.date.get(5..).unwrap_or(&e.date)}" }
                                span { class: "ph-text", "{e.text}" }
                            }
                        }
                    },
                }
            }
            h2 { class: "pg-h", "Coming next" }
            div { class: "ph-tiles",
                for (name, line) in [
                    ("Visualization board", "Your gallery for the morning read."),
                    ("Version I want to be", "Swipe cards with progress."),
                    ("New Me", "Editable cards of who you are becoming."),
                    ("Mind OS", "A second brain for links, notes and images."),
                ] {
                    div { key: "{name}", class: "ph-tile ms-next",
                        span { class: "ms-next-name", "{name}" }
                        span { class: "ms-next-line", "{line}" }
                    }
                }
            }
        }
    }
}

#[component]
fn Page(title: &'static str, lead: &'static str, children: Element) -> Element {
    rsx! {
        main { class: "page",
            div { class: "amb amb-quiet", aria_hidden: "true" }
            header { class: "page-head",
                h1 { class: "page-title", "{title}" }
                p { class: "page-lead", "{lead}" }
            }
            {children}
        }
    }
}
