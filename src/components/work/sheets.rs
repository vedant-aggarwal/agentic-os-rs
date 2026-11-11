//! "New thread" and "Launch several" sheets. Both share the session settings block: project
//! (or a new folder), provider, model, runtime mode, plan.

use aos_work::types::*;
use dioxus::prelude::*;

use super::{WI, WIcon, WorkSheet};
use crate::pages::work::{WorkUi, api};

#[derive(Clone, Copy, PartialEq)]
struct Settings {
    project: Signal<String>,
    folder: Signal<String>,
    provider: Signal<Provider>,
    model: Signal<String>,
    mode: Signal<RuntimeMode>,
    plan: Signal<bool>,
}

fn use_settings() -> Settings {
    let ui = use_context::<WorkUi>();
    let first = ui.filter.peek().clone().or_else(|| ui.projects.peek().first().map(|p| p.id.clone()));
    Settings {
        project: use_signal(|| first.unwrap_or_default()),
        folder: use_signal(String::new),
        provider: use_signal(|| Provider::Claude),
        model: use_signal(String::new),
        mode: use_signal(|| RuntimeMode::Supervised),
        plan: use_signal(|| false),
    }
}

impl Settings {
    /// (project, folder) for the request: a typed folder wins.
    fn target(&self) -> (Option<String>, Option<String>) {
        let folder = self.folder.peek().trim().to_string();
        if !folder.is_empty() {
            (None, Some(folder))
        } else {
            let p = self.project.peek().clone();
            (Some(p).filter(|s| !s.is_empty()), None)
        }
    }
}

#[component]
fn SettingsBlock(s: Settings) -> Element {
    let ui = use_context::<WorkUi>();
    let mut s = s;
    let projects = ui.projects.read().clone();
    let provider = *s.provider.read();
    rsx! {
        div { class: "wk-form",
            label { class: "wk-field",
                span { "Project" }
                select { value: "{s.project}", onchange: move |e| s.project.set(e.value()),
                    for p in projects {
                        option { key: "{p.id}", value: "{p.id}", selected: *s.project.read() == p.id, "{p.name}" }
                    }
                }
            }
            label { class: "wk-field",
                span { "Or a folder path" }
                div { class: "wk-folder",
                    WI { name: WIcon::Folder, size: 16 }
                    input { r#type: "text", placeholder: "C:\\path\\to\\repo", value: "{s.folder}", oninput: move |e| s.folder.set(e.value()) }
                }
            }
            div { class: "wk-field",
                span { "Agent" }
                div { class: "wk-seg", role: "radiogroup",
                    for p in Provider::ALL {
                        button {
                            key: "{p.as_str()}",
                            class: if provider == p { "on" } else { "" },
                            r#type: "button",
                            role: "radio",
                            aria_checked: provider == p,
                            onclick: move |_| { s.provider.set(p); s.model.set(String::new()); },
                            "{p.label()}"
                        }
                    }
                }
            }
            div { class: "wk-row2",
                label { class: "wk-field",
                    span { "Model" }
                    select { onchange: move |e| s.model.set(e.value()),
                        for (id, label) in provider.models().iter() {
                            option { key: "{id}", value: "{id}", selected: *s.model.read() == *id, "{label}" }
                        }
                    }
                }
                label { class: "wk-field",
                    span { "Access" }
                    select { onchange: move |e| s.mode.set(RuntimeMode::parse(&e.value()).unwrap_or_default()),
                        for m in RuntimeMode::ALL {
                            option { key: "{m.as_str()}", value: "{m.as_str()}", selected: *s.mode.read() == m, "{m.label()}" }
                        }
                    }
                }
            }
            label { class: "wk-check",
                input { r#type: "checkbox", checked: *s.plan.read(), onchange: move |e| s.plan.set(e.checked()) }
                span { "Plan first, then wait for my go" }
            }
        }
    }
}

#[component]
fn SheetFrame(title: &'static str, children: Element) -> Element {
    let mut ui = use_context::<WorkUi>();
    rsx! {
        div { class: "wk-scrim", onclick: move |_| ui.sheet.set(WorkSheet::None),
            div { class: "wk-sheet", role: "dialog", aria_modal: "true", aria_label: title, onclick: move |e| e.stop_propagation(),
                div { class: "wk-sheet-head",
                    h2 { "{title}" }
                    button { class: "wk-icon-btn", r#type: "button", aria_label: "Close", onclick: move |_| ui.sheet.set(WorkSheet::None),
                        WI { name: WIcon::Close, size: 18 }
                    }
                }
                {children}
            }
        }
    }
}

#[component]
pub fn NewThreadSheet() -> Element {
    let mut ui = use_context::<WorkUi>();
    let s = use_settings();
    let mut prompt = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut err = use_signal(|| None::<String>);

    let start = move |_| {
        if *busy.peek() {
            return;
        }
        let (project, folder) = s.target();
        let req = NewThread {
            project,
            folder,
            provider: Some(s.provider.peek().as_str().into()),
            model: Some(s.model.peek().clone()),
            runtime_mode: Some(s.mode.peek().as_str().into()),
            interaction_mode: Some(if *s.plan.peek() { "plan" } else { "default" }.into()),
            title: None,
            prompt: prompt.peek().clone(),
            origin: None,
        };
        busy.set(true);
        spawn(async move {
            match api::work_create(req).await {
                Ok(t) => {
                    ui.select(Some(t.id));
                    ui.sheet.set(WorkSheet::None);
                }
                Err(e) => err.set(Some(e.to_string())),
            }
            busy.set(false);
        });
    };

    rsx! {
        SheetFrame { title: "New thread",
            SettingsBlock { s }
            label { class: "wk-field",
                span { "First message" }
                textarea { class: "wk-area", rows: "4", placeholder: "What should it do?", value: "{prompt}", oninput: move |e| prompt.set(e.value()) }
            }
            if let Some(e) = err() { p { class: "wk-banner", "{e}" } }
            div { class: "wk-sheet-foot",
                button { class: "wk-btn wk-btn-primary wk-wide", r#type: "button", disabled: busy() || prompt.read().trim().is_empty(), onclick: start,
                    if busy() { "Starting…" } else { "Start" }
                }
            }
        }
    }
}

#[component]
pub fn LaunchSheet() -> Element {
    let mut ui = use_context::<WorkUi>();
    let s = use_settings();
    let mut tasks = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut err = use_signal(|| None::<String>);
    let count = tasks.read().lines().filter(|l| !l.trim().is_empty()).count();

    let launch = move |_| {
        if *busy.peek() {
            return;
        }
        let (project, folder) = s.target();
        let list: Vec<LaunchTask> = tasks
            .peek()
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(|l| LaunchTask { prompt: l.to_string(), ..Default::default() })
            .collect();
        let req = LaunchMany {
            project,
            folder,
            provider: Some(s.provider.peek().as_str().into()),
            model: Some(s.model.peek().clone()),
            runtime_mode: Some(s.mode.peek().as_str().into()),
            interaction_mode: Some(if *s.plan.peek() { "plan" } else { "default" }.into()),
            tasks: list,
        };
        busy.set(true);
        spawn(async move {
            match api::work_launch(req).await {
                Ok((started, failed)) => {
                    if failed.is_empty() {
                        ui.sheet.set(WorkSheet::None);
                        if let Some(first) = started.first() {
                            ui.filter.set(Some(first.project_id.clone()));
                        }
                    } else {
                        err.set(Some(format!("{} started, {} failed: {}", started.len(), failed.len(), failed.join("; "))));
                    }
                }
                Err(e) => err.set(Some(e.to_string())),
            }
            busy.set(false);
        });
    };

    rsx! {
        SheetFrame { title: "Launch several",
            SettingsBlock { s }
            label { class: "wk-field",
                span { "Tasks, one per line" }
                textarea { class: "wk-area", rows: "6", placeholder: "Fix the login redirect\nAdd tests for the parser\nWrite the README section", value: "{tasks}", oninput: move |e| tasks.set(e.value()) }
            }
            if let Some(e) = err() { p { class: "wk-banner", "{e}" } }
            div { class: "wk-sheet-foot",
                button { class: "wk-btn wk-btn-primary wk-wide", r#type: "button", disabled: busy() || count == 0, onclick: launch,
                    if busy() { "Launching…" } else if count == 1 { "Launch 1 session" } else { "Launch {count} sessions" }
                }
            }
        }
    }
}
