//! Today, the home. Top bar, the hero, the stats under it, mood and energy,
//! shortcuts, then today's list. One DOM for every width: on desktop the hero sits in
//! a floating centre panel and Nora + today move to a frosted right panel.

mod below;
mod goal_page;
pub mod hero;
mod today_list;
pub mod topbar;

use dioxus::prelude::*;

use crate::app::{Mode, Switch};
use crate::data::goal::Goal;
use crate::live;
use crate::state::{AppState, SheetKind};

use hero::HeroStage;

#[component]
pub fn Today(lock: Option<Switch>, mode: Option<Mode>) -> Element {
    // Read once by the shell when the app starts.
    let _ = (lock, mode);
    let mut state = use_context::<AppState>();
    let goal = (state.goal)();
    let pillar = (state.pillar)();
    let safe = (state.safe)();

    rsx! {
        main {
            class: "home",
            class: if goal.is_some() { "is-goal" },
            class: if pillar.is_some() { "is-pillar" },
            "data-front-scope": "1",
            "data-front": "me",
            "data-focus": goal.map(Goal::slug).unwrap_or(""),
            Ambient {}
            div { class: "home-main",
                topbar::TopBar {}
                HeroStage {
                    level: live::level().level,
                    private_mode: !safe,
                    focused: goal,
                    on_goal_tap: move |g: Goal| state.tap_goal(g),
                    on_self_tap: move |_| state.open_sheet(SheetKind::Levels),
                }
                div { class: "below",
                    below::Stats {}
                    below::MoodControl {}
                    below::Shortcuts {}
                    div { class: "phone-only", today_list::TodayCard {} }
                }
            }
            aside { class: "home-side", aria_label: "Nora and today",
                below::NoraCard {}
                today_list::TodayCard {}
            }
            goal_page::GoalPage {}
            goal_page::PillarPage {}
        }
    }
}

/// Light from the hero, spilling onto the page. One layer per hue; CSS fades between
/// them by what stands in front (or what is focused).
#[component]
fn Ambient() -> Element {
    rsx! {
        div { class: "amb", aria_hidden: "true",
            for k in ["me", "physical", "money", "mind"] {
                div { key: "{k}", class: "amb-l amb-{k}" }
            }
        }
    }
}
