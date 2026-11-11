//! The level roadmap. Each milestone changes his character (and, in private mode,
//! how clearly the companion shows). Looks are the generated stages in assets/img.

/// Highest level on the roadmap for now.
pub const TOP: u8 = 20;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Look {
    L1,
    L5,
    L10,
    L20,
}

impl Look {
    /// The look he has at a level: the last milestone reached.
    pub fn at(level: u8) -> Look {
        match level {
            0..=4 => Look::L1,
            5..=9 => Look::L5,
            10..=19 => Look::L10,
            _ => Look::L20,
        }
    }
}

pub struct Milestone {
    pub level: u8,
    pub look: Look,
    pub title: &'static str,
    pub detail: &'static str,
    /// Shown only in private mode.
    pub companion: Option<&'static str>,
}

pub const MILESTONES: &[Milestone] = &[
    Milestone {
        level: 1,
        look: Look::L1,
        title: "Day one",
        detail: "Where the story starts.",
        companion: Some("She is a faint outline."),
    },
    Milestone {
        level: 5,
        look: Look::L5,
        title: "Leaner",
        detail: "Sharper face, fitted hoodie.",
        companion: Some("Her colour starts to come in."),
    },
    Milestone {
        level: 10,
        look: Look::L10,
        title: "Fit",
        detail: "Athletic build, new wardrobe.",
        companion: Some("Fully there, in colour."),
    },
    Milestone {
        level: 20,
        look: Look::L20,
        title: "Peak",
        detail: "Long hair, long coat, the full aura.",
        companion: Some("Vivid, beside you."),
    },
];

pub fn milestone(level: u8) -> Option<&'static Milestone> {
    MILESTONES.iter().find(|m| m.level == level)
}

pub fn next_milestone(level: u8) -> Option<&'static Milestone> {
    MILESTONES.iter().find(|m| m.level > level)
}

/// 0 at level 1, 1 at the top: how vivid the companion is.
pub fn companion_vivid(level: u8) -> f32 {
    ((level.max(1) - 1) as f32 / (TOP - 1) as f32).clamp(0.0, 1.0)
}

/// XP still needed for the next level, from the server's ledger.
pub fn xp_left() -> u32 {
    crate::live::level().to_next.max(0) as u32
}
