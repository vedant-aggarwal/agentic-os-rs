//! Tap versus hold, measured between pointer down and up. The action fires on release,
//! so a scroll (which cancels the pointer) never ticks anything by accident.

use dioxus::prelude::*;
use web_time::Instant;

const HOLD_MS: u128 = 450;

#[derive(Clone, Copy)]
pub struct Press {
    started: Signal<Option<Instant>>,
}

pub fn use_press() -> Press {
    Press {
        started: use_signal(|| None),
    }
}

impl Press {
    pub fn down(&mut self) {
        self.started.set(Some(Instant::now()));
    }

    pub fn cancel(&mut self) {
        self.started.set(None);
    }

    /// `Some(true)` for a hold, `Some(false)` for a tap, `None` if the press began elsewhere.
    pub fn up(&mut self) -> Option<bool> {
        let started = self.started.take()?;
        Some(started.elapsed().as_millis() >= HOLD_MS)
    }
}
