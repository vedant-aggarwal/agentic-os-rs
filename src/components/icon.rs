//! Inline SVG icons. Paths are from Lucide (ISC licence). Stroke follows the text
//! beside it: 1.5 next to regular text, 2 next to semibold.

use dioxus::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IconName {
    Check,
    X,
    ChevronLeft,
    ChevronRight,
    Mic,
    MicOff,
    Phone,
    PhoneOff,
    Speaker,
    Message,
    Waves,
    Printer,
    Rocket,
    Sun,
    Inbox,
    Sprout,
    Terminal,
    Mountain,
    User,
    Shield,
    EyeOff,
    Lock,
    Send,
    Sparkle,
    Plus,
}

#[component]
pub fn Icon(
    name: IconName,
    #[props(default = 20)] size: u32,
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
            class: "icon",
            {paths(name)}
        }
    }
}

fn paths(name: IconName) -> Element {
    match name {
        IconName::Check => rsx! { path { d: "M20 6 9 17l-5-5" } },
        IconName::X => rsx! {
            path { d: "M18 6 6 18" }
            path { d: "m6 6 12 12" }
        },
        IconName::ChevronLeft => rsx! { path { d: "m15 18-6-6 6-6" } },
        IconName::ChevronRight => rsx! { path { d: "m9 18 6-6-6-6" } },
        IconName::Mic => rsx! {
            path { d: "M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3Z" }
            path { d: "M19 10v2a7 7 0 0 1-14 0v-2" }
            path { d: "M12 19v3" }
        },
        IconName::MicOff => rsx! {
            path { d: "M2 2l20 20" }
            path { d: "M18.89 13.23A7.12 7.12 0 0 0 19 12v-2" }
            path { d: "M5 10v2a7 7 0 0 0 12 5" }
            path { d: "M15 9.34V5a3 3 0 0 0-5.68-1.33" }
            path { d: "M9 9v3a3 3 0 0 0 5.12 2.12" }
            path { d: "M12 19v3" }
        },
        IconName::Phone => rsx! {
            path { d: "M22 16.92v3a2 2 0 0 1-2.18 2 19.79 19.79 0 0 1-8.63-3.07 19.5 19.5 0 0 1-6-6 19.79 19.79 0 0 1-3.07-8.67A2 2 0 0 1 4.11 2h3a2 2 0 0 1 2 1.72 12.84 12.84 0 0 0 .7 2.81 2 2 0 0 1-.45 2.11L8.09 9.91a16 16 0 0 0 6 6l1.27-1.27a2 2 0 0 1 2.11-.45 12.84 12.84 0 0 0 2.81.7A2 2 0 0 1 22 16.92z" }
        },
        IconName::PhoneOff => rsx! {
            path { d: "M10.1 13.9a14 14 0 0 0 3.732 2.668 1 1 0 0 0 1.213-.303l.355-.465A2 2 0 0 1 17 15h3a2 2 0 0 1 2 2v3a2 2 0 0 1-2 2 18 18 0 0 1-12.728-5.272" }
            path { d: "M22 2 2 22" }
            path { d: "M4.76 13.582A18 18 0 0 1 2 4a2 2 0 0 1 2-2h3a2 2 0 0 1 2 2v3a2 2 0 0 1-.8 1.6l-.468.351a1 1 0 0 0-.292 1.233 14 14 0 0 0 .244.473" }
        },
        IconName::Speaker => rsx! {
            path { d: "M11 4.702a.705.705 0 0 0-1.203-.498L6.413 7.587A1.4 1.4 0 0 1 5.416 8H3a1 1 0 0 0-1 1v6a1 1 0 0 0 1 1h2.416a1.4 1.4 0 0 1 .997.413l3.383 3.384A.705.705 0 0 0 11 19.298z" }
            path { d: "M16 9a5 5 0 0 1 0 6" }
            path { d: "M19.364 18.364a9 9 0 0 0 0-12.728" }
        },
        IconName::Message => rsx! { path { d: "M7.9 20A9 9 0 1 0 4 16.1L2 22Z" } },
        IconName::Waves => rsx! {
            path { d: "M2 6c.6.5 1.2 1 2.5 1C7 7 7 5 9.5 5c2.6 0 2.4 2 5 2 2.5 0 2.5-2 5-2 1.3 0 1.9.5 2.5 1" }
            path { d: "M2 12c.6.5 1.2 1 2.5 1 2.5 0 2.5-2 5-2 2.6 0 2.4 2 5 2 2.5 0 2.5-2 5-2 1.3 0 1.9.5 2.5 1" }
            path { d: "M2 18c.6.5 1.2 1 2.5 1 2.5 0 2.5-2 5-2 2.6 0 2.4 2 5 2 2.5 0 2.5-2 5-2 1.3 0 1.9.5 2.5 1" }
        },
        IconName::Printer => rsx! {
            path { d: "M6 18H4a2 2 0 0 1-2-2v-5a2 2 0 0 1 2-2h16a2 2 0 0 1 2 2v5a2 2 0 0 1-2 2h-2" }
            path { d: "M6 9V3a1 1 0 0 1 1-1h10a1 1 0 0 1 1 1v6" }
            rect { x: "6", y: "14", width: "12", height: "8", rx: "1" }
        },
        IconName::Rocket => rsx! {
            path { d: "M4.5 16.5c-1.5 1.26-2 5-2 5s3.74-.5 5-2c.71-.84.7-2.13-.09-2.91a2.18 2.18 0 0 0-2.91-.09z" }
            path { d: "m12 15-3-3a22 22 0 0 1 2-3.95A12.88 12.88 0 0 1 22 2c0 2.72-.78 7.5-6 11a22.35 22.35 0 0 1-4 2z" }
            path { d: "M9 12H4s.55-3.03 2-4c1.62-1.08 5 0 5 0" }
            path { d: "M12 15v5s3.03-.55 4-2c1.08-1.62 0-5 0-5" }
        },
        IconName::Sun => rsx! {
            circle { cx: "12", cy: "12", r: "4" }
            path { d: "M12 2v2" }
            path { d: "M12 20v2" }
            path { d: "m4.93 4.93 1.41 1.41" }
            path { d: "m17.66 17.66 1.41 1.41" }
            path { d: "M2 12h2" }
            path { d: "M20 12h2" }
            path { d: "m6.34 17.66-1.41 1.41" }
            path { d: "m19.07 4.93-1.41 1.41" }
        },
        IconName::Inbox => rsx! {
            path { d: "M22 12h-6l-2 3h-4l-2-3H2" }
            path { d: "M5.45 5.11 2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.45-6.89A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11z" }
        },
        IconName::Sprout => rsx! {
            path { d: "M7 20h10" }
            path { d: "M10 20c5.5-2.5.8-6.4 3-10" }
            path { d: "M9.5 9.4c1.1.8 1.8 2.2 2.3 3.7-2 .4-3.5.4-4.8-.3-1.2-.6-2.3-1.9-3-4.2 2.8-.5 4.4 0 5.5.8z" }
            path { d: "M14.1 6a7 7 0 0 0-1.1 4c1.9-.1 3.3-.6 4.3-1.4 1-1 1.6-2.3 1.7-4.6-2.7.1-4 1-4.9 2z" }
        },
        IconName::Terminal => rsx! {
            path { d: "m4 17 6-6-6-6" }
            path { d: "M12 19h8" }
        },
        IconName::Mountain => rsx! { path { d: "m8 3 4 8 5-5 5 15H2L8 3z" } },
        IconName::User => rsx! {
            circle { cx: "12", cy: "8", r: "4.5" }
            path { d: "M20 21a8 8 0 0 0-16 0" }
        },
        IconName::Shield => rsx! {
            path { d: "M20 13c0 5-3.5 7.5-7.66 8.95a1 1 0 0 1-.67-.01C7.5 20.5 4 18 4 13V6a1 1 0 0 1 1-1c2 0 4.5-1.2 6.24-2.72a1.17 1.17 0 0 1 1.52 0C14.51 3.81 17 5 19 5a1 1 0 0 1 1 1z" }
        },
        IconName::EyeOff => rsx! {
            path { d: "M10.733 5.076a10.744 10.744 0 0 1 11.205 6.575 1 1 0 0 1 0 .696 10.747 10.747 0 0 1-1.444 2.49" }
            path { d: "M14.084 14.158a3 3 0 0 1-4.242-4.242" }
            path { d: "M17.479 17.499a10.75 10.75 0 0 1-15.417-5.151 1 1 0 0 1 0-.696 10.75 10.75 0 0 1 4.446-5.143" }
            path { d: "m2 2 20 20" }
        },
        IconName::Lock => rsx! {
            rect { width: "18", height: "11", x: "3", y: "11", rx: "2", ry: "2" }
            path { d: "M7 11V7a5 5 0 0 1 10 0v4" }
        },
        IconName::Send => rsx! {
            path { d: "M14.536 21.686a.5.5 0 0 0 .937-.024l6.5-19a.496.496 0 0 0-.635-.635l-19 6.5a.5.5 0 0 0-.024.937l7.93 3.18a2 2 0 0 1 1.112 1.11z" }
            path { d: "m21.854 2.147-10.94 10.939" }
        },
        IconName::Sparkle => rsx! {
            path { d: "M9.937 15.5A2 2 0 0 0 8.5 14.063l-6.135-1.582a.5.5 0 0 1 0-.962L8.5 9.936A2 2 0 0 0 9.937 8.5l1.582-6.135a.5.5 0 0 1 .963 0L14.063 8.5A2 2 0 0 0 15.5 9.937l6.135 1.581a.5.5 0 0 1 0 .964L15.5 14.063a2 2 0 0 0-1.437 1.437l-1.582 6.135a.5.5 0 0 1-.963 0z" }
        },
        IconName::Plus => rsx! {
            path { d: "M5 12h14" }
            path { d: "M12 5v14" }
        },
    }
}
