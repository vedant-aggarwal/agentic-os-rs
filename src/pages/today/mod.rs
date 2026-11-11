//! The home page. One DOM for every width: CSS flattens the three columns on phones.

mod core_six;
mod daygrid;
mod header;
mod needs;
mod sheets;
mod side;
mod state_card;
mod three;
mod timeline;
mod week;

use dioxus::prelude::*;

use crate::components::CaptureBar;
use crate::data::DayState;
use crate::state::TodayState;
use crate::theme::ThemePref;

use core_six::CoreSix;
use daygrid::DayGrid;
use header::Header;
use needs::NeedsYou;
use sheets::TodaySheets;
use side::{NoraPanel, WorkPanel};
use state_card::StateCard;
use three::TodaysThree;
use timeline::TimelineList;
use week::WeekStrip;

#[component]
pub fn Today(state: Option<DayState>, theme: Option<ThemePref>) -> Element {
    let _ = theme; // applied by the shell
    let state = state.unwrap_or_default();
    rsx! {
        // Keyed so a state switch starts from that state's sample ticks.
        TodayPage { key: "{state}", state }
    }
}

#[component]
fn TodayPage(state: DayState) -> Element {
    use_context_provider(|| TodayState::new(state));

    rsx! {
        main { class: "page state-{state}",
            aside { class: "col col-cal", DayGrid {} }
            section { class: "col col-day",
                Header {}
                div { class: "capture-top", CaptureBar {} }
                StateCard {}
                TodaysThree {}
                CoreSix {}
                TimelineList {}
                WeekStrip {}
            }
            aside { class: "col col-side",
                NoraPanel {}
                NeedsYou {}
                WorkPanel {}
            }
            TodaySheets {}
        }
    }
}
