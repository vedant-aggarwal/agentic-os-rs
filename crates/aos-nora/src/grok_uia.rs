//! Grok Bot write side through Windows UI Automation (the accessibility tree; the app runs with
//! `--force-renderer-accessibility`). No screen coordinates and no mouse: the window is found by
//! class and title, the chat is chosen by invoking its sidebar item, the composer is the `Edit`
//! named "Prompt", the text goes in through the value pattern (typed keys as a fallback) and is
//! sent by invoking the Send button (Enter as a fallback).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use uiautomation::controls::ControlType;
use uiautomation::patterns::{UIInvokePattern, UIValuePattern, UIWindowPattern};
use uiautomation::types::WindowVisualState;
use uiautomation::{UIAutomation, UIElement};

use crate::grok::{GrokWriter, WriteError};

#[link(name = "user32")]
unsafe extern "system" {
    fn OpenInputDesktop(flags: u32, inherit: i32, access: u32) -> isize;
    fn CloseDesktop(h: isize) -> i32;
}

/// True while the interactive desktop is not reachable (lock screen, secure desktop).
pub fn session_locked() -> bool {
    // SAFETY: plain Win32 calls with no pointers; the handle is closed right away.
    unsafe {
        let h = OpenInputDesktop(0, 0, 0x0100); // DESKTOP_SWITCHDESKTOP
        if h == 0 {
            return true;
        }
        CloseDesktop(h);
    }
    false
}

pub struct UiaWriter {
    /// Sidebar label of Nora's chat (`Nora`); matched against the start of the item's name.
    pub chat_label: String,
    pub exe: PathBuf,
    pub launch_wait: Duration,
}

impl UiaWriter {
    pub fn new(chat_label: &str) -> UiaWriter {
        let exe = std::env::var_os("AOS_GROK_EXE")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Program Files\Grok Bot\Grok Bot.exe"));
        UiaWriter { chat_label: chat_label.to_string(), exe, launch_wait: Duration::from_secs(60) }
    }
}

fn fail<E: std::fmt::Display>(what: &'static str) -> impl Fn(E) -> WriteError {
    move |e| WriteError::Failed(format!("{what}: {e}"))
}

fn find_window(a: &UIAutomation) -> Option<UIElement> {
    let root = a.get_root_element().ok()?;
    a.create_matcher().from(root).depth(2).timeout(0).classname("Chrome_WidgetWin_1").name("Grok Bot").find_first().ok()
}

fn running() -> bool {
    std::process::Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq Grok Bot.exe", "/NH"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("Grok Bot.exe"))
        .unwrap_or(false)
}

fn descendants(a: &UIAutomation, w: &UIElement, ct: ControlType) -> Vec<UIElement> {
    a.create_matcher().from_ref(w).timeout(0).depth(30).control_type(ct).find_all().unwrap_or_default()
}

fn composer(a: &UIAutomation, w: &UIElement) -> Option<UIElement> {
    a.create_matcher().from_ref(w).timeout(0).depth(30).control_type(ControlType::Edit).name("Prompt").find_first().ok()
}

fn heading(a: &UIAutomation, w: &UIElement) -> Option<String> {
    a.create_matcher().from_ref(w).timeout(0).depth(30).filter_fn(Box::new(|e: &UIElement| {
        Ok(e.get_automation_id().is_ok_and(|id| id == "sand-conversation-heading"))
    }))
    .find_first()
    .ok()?
    .get_name()
    .ok()
}

fn label_matches(name: &str, label: &str) -> bool {
    let (n, l) = (name.trim().to_lowercase(), label.trim().to_lowercase());
    n.strip_prefix(&l).is_some_and(|rest| rest.is_empty() || !rest.starts_with(|c: char| c.is_alphanumeric()))
}

impl UiaWriter {
    /// Window up, launching the app if needed.
    fn window(&self, a: &UIAutomation) -> Result<UIElement, WriteError> {
        if let Some(w) = find_window(a) {
            return Ok(w);
        }
        // A second launch of a running Electron app hands over to the first instance.
        let _ = running();
        std::process::Command::new(&self.exe)
            .arg("--force-renderer-accessibility")
            .spawn()
            .map_err(|e| WriteError::Failed(format!("cannot start Grok Bot: {e}")))?;
        let end = Instant::now() + self.launch_wait;
        while Instant::now() < end {
            std::thread::sleep(Duration::from_secs(2));
            if let Some(w) = find_window(a)
                && composer(a, &w).is_some()
            {
                return Ok(w);
            }
        }
        Err(WriteError::NotRunning("Grok Bot window did not appear".into()))
    }

    fn select_chat(&self, a: &UIAutomation, w: &UIElement) -> Result<(), WriteError> {
        if heading(a, w).is_some_and(|h| label_matches(&h, &self.chat_label)) {
            return Ok(());
        }
        let mut items = descendants(a, w, ControlType::Button);
        items.extend(descendants(a, w, ControlType::ListItem));
        let target = items
            .into_iter()
            .find(|e| e.get_name().is_ok_and(|n| label_matches(&n, &self.chat_label)))
            .ok_or_else(|| WriteError::Failed(format!("chat '{}' not in the sidebar", self.chat_label)))?;
        match target.get_pattern::<UIInvokePattern>() {
            Ok(p) => p.invoke().map_err(fail("invoke chat"))?,
            Err(_) => {
                target.set_focus().map_err(fail("focus chat"))?;
                target.send_keys("{enter}", 0).map_err(fail("open chat"))?;
            }
        }
        let end = Instant::now() + Duration::from_secs(8);
        while Instant::now() < end {
            std::thread::sleep(Duration::from_millis(300));
            if heading(a, w).is_some_and(|h| label_matches(&h, &self.chat_label)) {
                return Ok(());
            }
        }
        Err(WriteError::Failed("chat did not open".into()))
    }
}

impl GrokWriter for UiaWriter {
    fn send(&self, text: &str) -> Result<(), WriteError> {
        if session_locked() {
            return Err(WriteError::Locked);
        }
        let a = UIAutomation::new().map_err(fail("UI Automation"))?;
        let w = self.window(&a)?;
        if let Ok(p) = w.get_pattern::<UIWindowPattern>()
            && p.get_window_visual_state().ok() == Some(WindowVisualState::Minimized)
        {
            let _ = p.set_window_visual_state(WindowVisualState::Normal);
            std::thread::sleep(Duration::from_millis(600));
        }
        let _ = w.set_focus();
        self.select_chat(&a, &w)?;
        // Grok still replying: a Stop button shows next to the composer.
        let stop = descendants(&a, &w, ControlType::Button)
            .into_iter()
            .any(|b| b.get_name().is_ok_and(|n| n.to_lowercase().starts_with("stop")));
        if stop {
            return Err(WriteError::Busy("Grok is replying".into()));
        }
        let edit = composer(&a, &w).ok_or_else(|| WriteError::Failed("composer not found".into()))?;
        let value = edit.get_pattern::<UIValuePattern>().ok();
        if let Some(v) = &value
            && v.get_value().is_ok_and(|t| !t.trim().is_empty())
        {
            return Err(WriteError::Busy("composer has a draft in it".into()));
        }
        edit.set_focus().map_err(fail("focus composer"))?;
        let mut set = false;
        if let Some(v) = &value
            && v.set_value(text).is_ok()
        {
            std::thread::sleep(Duration::from_millis(250));
            let head: String = text.chars().take(20).collect();
            set = v.get_value().is_ok_and(|t| t.contains(&head));
        }
        if !set {
            edit.send_text(text, 5).map_err(fail("type text"))?;
            std::thread::sleep(Duration::from_millis(250));
        }
        // Send: the Send button if the tree has one, else Enter in the focused composer.
        let send_btn = descendants(&a, &w, ControlType::Button)
            .into_iter()
            .find(|b| b.get_name().is_ok_and(|n| n.eq_ignore_ascii_case("send") || n.to_lowercase().starts_with("send message")));
        match send_btn.and_then(|b| b.get_pattern::<UIInvokePattern>().ok()) {
            Some(p) => p.invoke().map_err(fail("press Send"))?,
            None => {
                edit.set_focus().map_err(fail("refocus composer"))?;
                edit.send_keys("{enter}", 0).map_err(fail("press Enter"))?;
            }
        }
        Ok(())
    }

    fn describe(&self) -> String {
        "uia".into()
    }
}

/// One line per element, for finding names and ids (used by the `grok_probe` example).
pub fn dump_tree() -> Result<Vec<String>, WriteError> {
    let a = UIAutomation::new().map_err(fail("UI Automation"))?;
    let w = find_window(&a).ok_or_else(|| WriteError::NotRunning("no Grok Bot window".into()))?;
    let walker = a.get_control_view_walker().map_err(fail("walker"))?;
    let mut out = Vec::new();
    fn go(wk: &uiautomation::UITreeWalker, e: &UIElement, d: usize, out: &mut Vec<String>) {
        if d > 20 || out.len() > 400 {
            return;
        }
        let name: String = e.get_name().unwrap_or_default().chars().take(70).collect();
        out.push(format!("{}{:?} '{}' id={}", "  ".repeat(d), e.get_control_type().ok(), name.replace('\n', " "), e.get_automation_id().unwrap_or_default()));
        let mut c = wk.get_first_child(e);
        while let Ok(ch) = c {
            go(wk, &ch, d + 1, out);
            c = wk.get_next_sibling(&ch);
        }
    }
    go(&walker, &w, 0, &mut out);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::label_matches;

    #[test]
    fn sidebar_labels() {
        assert!(label_matches("Nora · Always-on all-in-one assistant, Main Bot", "Nora"));
        assert!(label_matches("Nora", "nora"));
        assert!(!label_matches("Norah Jones", "Nora"));
        assert!(!label_matches("Munshi · Inbox", "Nora"));
    }
}
