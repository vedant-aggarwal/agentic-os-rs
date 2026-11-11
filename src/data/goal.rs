//! The three pillars and the goal that stands for each one in the hero.
//! The goals themselves (name, specs, price, earned) come from the profile: the server's
//! `profile` tool once paired, else the bundled `assets/personal/profile.json`, else the
//! generic defaults below. No personal values live in this file.

use dioxus::prelude::*;

use crate::live::{self, HeroGoal};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Goal {
    Physical,
    Money,
    Mind,
}

impl Goal {
    pub const ALL: [Goal; 3] = [Goal::Physical, Goal::Money, Goal::Mind];

    pub fn slug(self) -> &'static str {
        match self {
            Goal::Physical => "physical",
            Goal::Money => "money",
            Goal::Mind => "mind",
        }
    }

    pub fn from_slug(s: &str) -> Option<Goal> {
        match s {
            "physical" => Some(Goal::Physical),
            "money" => Some(Goal::Money),
            "mind" => Some(Goal::Mind),
            _ => None,
        }
    }

    /// The server's pillar id behind this hero slot.
    pub fn pillar_id(self) -> &'static str {
        match self {
            Goal::Physical => "body",
            Goal::Money => "wealth",
            Goal::Mind => "mind",
        }
    }

    pub fn from_pillar_id(id: &str) -> Option<Goal> {
        Goal::ALL.into_iter().find(|g| g.pillar_id() == id)
    }

    /// Pillar name, also the page title behind the goal.
    pub fn pillar(self) -> &'static str {
        match self {
            Goal::Physical => "Physical",
            Goal::Money => "Money",
            Goal::Mind => "Mind",
        }
    }

    /// The stat that levels with this pillar.
    pub fn stat(self) -> &'static str {
        match self {
            Goal::Physical => "Body",
            Goal::Money => "Work",
            Goal::Mind => "Mind",
        }
    }

    pub fn stat_level(self) -> u32 {
        live::pillar_level(self.pillar_id())
    }

    /// Mind's goal is the companion: private mode only.
    pub fn private(self) -> bool {
        self == Goal::Mind
    }

    /// The goal in this slot, from the current profile.
    pub fn spec(self) -> GoalSpec {
        let profile = live::PROFILE.read();
        GoalSpec::from_hero(self, profile.hero(self.slug()))
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct GoalSpec {
    pub name: String,
    /// One line under the name: variant and colour.
    pub variant: String,
    /// Rupees. `None` for goals that are not bought.
    pub price: Option<u32>,
    pub price_note: Option<String>,
    /// Rupees he has logged toward it. `None` until he starts logging.
    pub earned: Option<u32>,
    pub specs: Vec<(String, String)>,
    pub why: Option<String>,
    /// What the pillar covers.
    pub pillar_about: String,
}

impl GoalSpec {
    fn from_hero(goal: Goal, h: Option<&HeroGoal>) -> GoalSpec {
        let (name, variant, about) = match goal {
            Goal::Physical => ("Your body goal", "Set it in the profile on your server.", "Body, strength, sleep and how you carry yourself."),
            Goal::Money => ("Your money goal", "Set it in the profile on your server.", "Work and money: business, skills and every work session."),
            Goal::Mind => ("Companion", "Faint for now. She grows clearer as you level up.", "Personality, how you speak, confidence, knowledge and peace of mind."),
        };
        let default_specs: Vec<(String, String)> = if goal == Goal::Mind {
            [
                ("Personality", "Who you are when nobody is grading you"),
                ("Speaking", "How you talk and hold a room"),
                ("Knowledge", "What you read and learn"),
                ("Peace", "A calm mind, most days"),
            ]
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
        } else {
            Vec::new()
        };
        let Some(h) = h else {
            return GoalSpec {
                name: name.into(),
                variant: variant.into(),
                price: None,
                price_note: None,
                earned: None,
                specs: default_specs,
                why: None,
                pillar_about: about.into(),
            };
        };
        let specs: Vec<(String, String)> = h
            .specs
            .iter()
            .map(|line| match line.split_once(": ") {
                Some((k, v)) => (k.to_string(), v.to_string()),
                None => (String::new(), line.clone()),
            })
            .collect();
        GoalSpec {
            name: h.name.clone().unwrap_or_else(|| name.into()),
            variant: h.variant.clone().unwrap_or_default(),
            price: h.price.filter(|p| *p > 0).map(|p| p as u32),
            price_note: h.price_note.clone(),
            earned: h.earned.filter(|e| *e > 0).map(|e| e as u32),
            specs: if specs.is_empty() { default_specs } else { specs },
            why: h.why.clone(),
            pillar_about: h.about.clone().unwrap_or_else(|| about.into()),
        }
    }
}

/// Indian digit grouping: 541800 -> "5,41,800".
pub fn inr(n: u32) -> String {
    let s = n.to_string();
    if s.len() <= 3 {
        return s;
    }
    let (head, tail) = s.split_at(s.len() - 3);
    let mut out = String::new();
    let lead = head.len() % 2;
    for (i, c) in head.chars().enumerate() {
        if i > 0 && (i + 2 - lead) % 2 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    format!("{out},{tail}")
}

#[cfg(test)]
mod tests {
    use super::inr;

    #[test]
    fn groups_like_india() {
        assert_eq!(inr(999), "999");
        assert_eq!(inr(1_000), "1,000");
        assert_eq!(inr(364_856), "3,64,856");
        assert_eq!(inr(541_800), "5,41,800");
        assert_eq!(inr(12_345_678), "1,23,45,678");
    }
}
