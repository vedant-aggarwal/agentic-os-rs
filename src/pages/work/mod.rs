//! The Work tab: a T3 Code replacement. Projects and threads on the left, the thread in the
//! middle, the diff on the right (desktop). On phones the list and the thread are two views.
//!
//! Data: server functions in [`api`] for reads and actions, plus one live SSE stream of
//! `WorkEvent`s that patches the signals in [`WorkUi`]. Styles: `assets/work.css` (Aura tokens,
//! scoped to `.work-page`).

pub mod api;

use aos_work::types::*;
use dioxus::prelude::*;
use dioxus::core::{Runtime, ScopeId, current_scope_id};

use crate::components::work::{
    DiffPanel, LaunchSheet, NewThreadSheet, ThreadList, ThreadView, WorkSheet,
};

const WORK_CSS: Asset = asset!("/assets/work.css");

/// Everything the Work components share. `Copy`: signals only.
#[derive(Clone, Copy)]
pub struct WorkUi {
    pub projects: Signal<Vec<Project>>,
    pub threads: Signal<Vec<ThreadSummary>>,
    pub selected: Signal<Option<String>>,
    pub detail: Signal<Option<ThreadDetail>>,
    pub diff: Signal<Option<ThreadDiff>>,
    pub show_diff: Signal<bool>,
    pub sheet: Signal<WorkSheet>,
    /// Project filter for the list (`None` = all).
    pub filter: Signal<Option<String>>,
    /// The live stream is connected.
    pub live: Signal<bool>,
    pub error: Signal<Option<String>>,
    /// The Work page's scope: loads run there, so a sheet closing doesn't cancel them.
    pub scope: ScopeId,
}

impl WorkUi {
    fn spawn_here(&self, fut: impl std::future::Future<Output = ()> + 'static) {
        Runtime::current().spawn(self.scope, fut);
    }

    pub fn select(mut self, id: Option<String>) {
        self.selected.set(id.clone());
        self.detail.set(None);
        self.diff.set(None);
        if let Some(id) = id {
            self.load_detail(id);
        }
    }

    pub fn load_detail(mut self, id: String) {
        self.spawn_here(async move {
            match api::work_thread(id.clone()).await {
                Ok(d) if self.selected.peek().as_deref() == Some(id.as_str()) => self.detail.set(Some(d)),
                Ok(_) => {}
                Err(e) => self.error.set(Some(e.to_string())),
            }
        });
        self.load_diff();
    }

    pub fn load_diff(mut self) {
        let Some(id) = self.selected.peek().clone() else { return };
        self.spawn_here(async move {
            if let Ok(d) = api::work_diff(id.clone()).await {
                if self.selected.peek().as_deref() == Some(id.as_str()) {
                    self.diff.set(Some(d));
                }
            }
        });
    }

    pub fn reload_lists(mut self) {
        self.spawn_here(async move {
            match api::work_projects().await {
                Ok(p) => self.projects.set(p),
                Err(e) => self.error.set(Some(e.to_string())),
            }
            if let Ok(t) = api::work_threads().await {
                self.threads.set(t);
            }
        });
    }

    pub fn project(&self, id: &str) -> Option<Project> {
        self.projects.read().iter().find(|p| p.id == id).cloned()
    }

    /// Patch local state with one live event.
    fn apply(mut self, event: WorkEvent) {
        let selected = self.selected.peek().clone();
        let is_open = |tid: &str| selected.as_deref() == Some(tid);
        match event {
            WorkEvent::Hello { .. } | WorkEvent::Resync => {
                self.reload_lists();
                if let Some(id) = selected {
                    self.load_detail(id);
                }
            }
            WorkEvent::ProjectUpserted { project } => {
                let mut ps = self.projects.write();
                if !ps.iter().any(|p| p.id == project.id) {
                    ps.push(project);
                    ps.sort_by_key(|p| p.name.to_lowercase());
                }
            }
            WorkEvent::ThreadUpserted { thread } => {
                if is_open(&thread.id) {
                    if let Some(d) = self.detail.write().as_mut() {
                        d.thread = thread.clone();
                    }
                }
                let mut ts = self.threads.write();
                match ts.iter_mut().find(|t| t.id == thread.id) {
                    Some(t) => *t = thread,
                    None => ts.push(thread),
                }
                ts.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
            }
            WorkEvent::ItemUpserted { item } if is_open(&item.thread_id) => {
                if let Some(d) = self.detail.write().as_mut() {
                    match d.items.iter_mut().find(|i| i.id == item.id) {
                        Some(i) => *i = item,
                        None => {
                            d.items.push(item);
                            d.items.sort_by_key(|i| i.seq);
                        }
                    }
                }
            }
            WorkEvent::ItemDelta { thread_id, item_id, delta } if is_open(&thread_id) => {
                if let Some(d) = self.detail.write().as_mut() {
                    if let Some(i) = d.items.iter_mut().find(|i| i.id == item_id) {
                        i.text.push_str(&delta);
                    }
                }
            }
            WorkEvent::ApprovalRequested { approval } | WorkEvent::ApprovalResolved { approval }
                if is_open(&approval.thread_id) =>
            {
                if let Some(d) = self.detail.write().as_mut() {
                    match d.approvals.iter_mut().find(|a| a.id == approval.id) {
                        Some(a) => *a = approval,
                        None => d.approvals.push(approval),
                    }
                }
            }
            WorkEvent::TurnStarted { thread_id, turn } | WorkEvent::TurnCompleted { thread_id, turn }
                if is_open(&thread_id) =>
            {
                if let Some(d) = self.detail.write().as_mut() {
                    match d.turns.iter_mut().find(|t| t.id == turn.id) {
                        Some(t) => *t = turn,
                        None => d.turns.push(turn),
                    }
                }
            }
            WorkEvent::PlanUpdated { thread_id, steps } if is_open(&thread_id) => {
                if let Some(d) = self.detail.write().as_mut() {
                    d.plan = steps;
                }
            }
            WorkEvent::DiffChanged { thread_id } if is_open(&thread_id) => self.load_diff(),
            _ => {}
        }
    }
}

/// Wait `ms` on any platform (a JS timer through the webview / browser).
pub async fn sleep_ms(ms: u32) {
    let mut ev = document::eval(&format!("setTimeout(() => dioxus.send(1), {ms});"));
    let _ = ev.recv::<i32>().await;
}

#[component]
pub fn Work() -> Element {
    let ui = WorkUi {
        projects: use_signal(Vec::new),
        threads: use_signal(Vec::new),
        selected: use_signal(|| None),
        detail: use_signal(|| None),
        diff: use_signal(|| None),
        show_diff: use_signal(|| false),
        sheet: use_signal(|| WorkSheet::None),
        filter: use_signal(|| None),
        live: use_signal(|| false),
        error: use_signal(|| None),
        scope: current_scope_id(),
    };
    use_context_provider(|| ui);

    // The live stream (client only). Reconnects with a short pause; `Hello` triggers a refetch.
    use_future(move || async move {
        if cfg!(feature = "server") {
            return;
        }
        let mut ui = ui;
        loop {
            if let Ok(mut stream) = api::work_stream().await {
                ui.live.set(true);
                ui.error.set(None);
                while let Some(Ok(event)) = stream.recv().await {
                    ui.apply(event);
                }
            }
            ui.live.set(false);
            sleep_ms(2000).await;
        }
    });

    // `/work?t=<thread id>` opens that thread, `&diff=1` the diff (deep links, screenshots).
    use_effect(move || {
        let mut ui = ui;
        spawn(async move {
            let mut ev = document::eval(
                "const q = new URLSearchParams(location.search); dioxus.send([q.get('t') || '', q.get('diff') || '']);",
            );
            if let Ok((t, diff)) = ev.recv::<(String, String)>().await {
                if !t.is_empty() {
                    ui.select(Some(t));
                }
                if diff == "1" {
                    ui.show_diff.set(true);
                }
            }
        });
    });

    let view = if ui.selected.read().is_some() { "thread" } else { "list" };
    let diff_open = *ui.show_diff.read();
    let sheet = ui.sheet.read().clone();

    rsx! {
        document::Stylesheet { href: WORK_CSS }
        div { class: "work-page", "data-view": view, "data-diff": if diff_open { "open" } else { "closed" },
            ThreadList {}
            ThreadView {}
            DiffPanel {}
            match sheet {
                WorkSheet::None => rsx! {},
                WorkSheet::NewThread => rsx! { NewThreadSheet {} },
                WorkSheet::Launch => rsx! { LaunchSheet {} },
            }
        }
    }
}
