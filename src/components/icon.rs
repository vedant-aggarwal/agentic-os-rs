//! Inline SVG icons. Paths are from Lucide (ISC licence), drawn at 1.75 stroke.

use dioxus::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IconName {
    Check,
    X,
    Minus,
    ChevronDown,
    ChevronUp,
    ChevronRight,
    Mic,
    Phone,
    Message,
    Waves,
    Sunrise,
    Home,
    Briefcase,
    Brain,
    Send,
}

#[component]
pub fn Icon(
    name: IconName,
    #[props(default = 18)] size: u32,
    #[props(default = 1.75)] stroke: f32,
) -> Element {
    rsx! {
        svg {
            xmlns: "http://www.w3.org/2000/svg",
            view_box: "0 0 24 24",
            width: "{size}",
            height: "{size}",
            fill: "none",
            stroke: "currentColor",
            stroke_width: "{stroke}",
            stroke_linecap: "round",
            stroke_linejoin: "round",
            "aria-hidden": "true",
            {paths(name)}
        }
    }
}

fn paths(name: IconName) -> Element {
    match name {
        IconName::Check => rsx! {
            path { d: "M20 6 9 17l-5-5" }
        },
        IconName::X => rsx! {
            path { d: "M18 6 6 18" }
            path { d: "m6 6 12 12" }
        },
        IconName::Minus => rsx! {
            path { d: "M5 12h14" }
        },
        IconName::ChevronDown => rsx! {
            path { d: "m6 9 6 6 6-6" }
        },
        IconName::ChevronUp => rsx! {
            path { d: "m18 15-6-6-6 6" }
        },
        IconName::ChevronRight => rsx! {
            path { d: "m9 18 6-6-6-6" }
        },
        IconName::Mic => rsx! {
            path { d: "M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3Z" }
            path { d: "M19 10v2a7 7 0 0 1-14 0v-2" }
            path { d: "M12 19v3" }
        },
        IconName::Phone => rsx! {
            path { d: "M22 16.92v3a2 2 0 0 1-2.18 2 19.79 19.79 0 0 1-8.63-3.07 19.5 19.5 0 0 1-6-6 19.79 19.79 0 0 1-3.07-8.67A2 2 0 0 1 4.11 2h3a2 2 0 0 1 2 1.72 12.84 12.84 0 0 0 .7 2.81 2 2 0 0 1-.45 2.11L8.09 9.91a16 16 0 0 0 6 6l1.27-1.27a2 2 0 0 1 2.11-.45 12.84 12.84 0 0 0 2.81.7A2 2 0 0 1 22 16.92z" }
        },
        IconName::Message => rsx! {
            path { d: "M7.9 20A9 9 0 1 0 4 16.1L2 22Z" }
        },
        IconName::Waves => rsx! {
            path { d: "M2 6c.6.5 1.2 1 2.5 1C7 7 7 5 9.5 5c2.6 0 2.4 2 5 2 2.5 0 2.5-2 5-2 1.3 0 1.9.5 2.5 1" }
            path { d: "M2 12c.6.5 1.2 1 2.5 1 2.5 0 2.5-2 5-2 2.6 0 2.4 2 5 2 2.5 0 2.5-2 5-2 1.3 0 1.9.5 2.5 1" }
            path { d: "M2 18c.6.5 1.2 1 2.5 1 2.5 0 2.5-2 5-2 2.6 0 2.4 2 5 2 2.5 0 2.5-2 5-2 1.3 0 1.9.5 2.5 1" }
        },
        IconName::Sunrise => rsx! {
            path { d: "M12 2v8" }
            path { d: "m4.93 10.93 1.41 1.41" }
            path { d: "M2 18h2" }
            path { d: "M20 18h2" }
            path { d: "m19.07 10.93-1.41 1.41" }
            path { d: "M22 22H2" }
            path { d: "m8 6 4-4 4 4" }
            path { d: "M16 18a4 4 0 0 0-8 0" }
        },
        IconName::Home => rsx! {
            path { d: "M15 21v-8a1 1 0 0 0-1-1h-4a1 1 0 0 0-1 1v8" }
            path { d: "M3 10a2 2 0 0 1 .709-1.528l7-5.999a2 2 0 0 1 2.582 0l7 5.999A2 2 0 0 1 21 10v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" }
        },
        IconName::Briefcase => rsx! {
            path { d: "M16 20V4a2 2 0 0 0-2-2h-4a2 2 0 0 0-2 2v16" }
            rect { width: "20", height: "14", x: "2", y: "6", rx: "2" }
        },
        IconName::Brain => rsx! {
            path { d: "M12 5a3 3 0 1 0-5.997.125 4 4 0 0 0-2.526 5.77 4 4 0 0 0 .556 6.588A4 4 0 1 0 12 18Z" }
            path { d: "M12 5a3 3 0 1 1 5.997.125 4 4 0 0 1 2.526 5.77 4 4 0 0 1-.556 6.588A4 4 0 1 1 12 18Z" }
            path { d: "M15 13a4.5 4.5 0 0 1-3-4 4.5 4.5 0 0 1-3 4" }
            path { d: "M17.599 6.5a3 3 0 0 0 .399-1.375" }
            path { d: "M6.003 5.125A3 3 0 0 0 6.401 6.5" }
            path { d: "M3.477 10.896a4 4 0 0 1 .585-.396" }
            path { d: "M19.938 10.5a4 4 0 0 1 .585.396" }
            path { d: "M6 18a4 4 0 0 1-1.967-.516" }
            path { d: "M19.967 17.484A4 4 0 0 1 18 18" }
        },
        IconName::Send => rsx! {
            path { d: "M14.536 21.686a.5.5 0 0 0 .937-.024l6.5-19a.496.496 0 0 0-.635-.635l-19 6.5a.5.5 0 0 0-.024.937l7.93 3.18a2 2 0 0 1 1.112 1.11z" }
            path { d: "m21.854 2.147-10.94 10.939" }
        },
    }
}
