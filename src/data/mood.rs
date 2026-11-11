//! Mood and energy check-in: two scales, then the reasons behind them.

use super::Time;

pub const MOODS: [&str; 7] = ["Very low", "Low", "A bit low", "Okay", "Good", "Very good", "Great"];
pub const ENERGY: [&str; 5] = ["Drained", "Low", "Steady", "Good", "Charged"];

/// Orb colour per mood step, cool to warm (Apple's State of Mind runs purple to orange).
pub const MOOD_HUES: [&str; 7] = [
    "#6c5cff", "#4e7cff", "#5f9fd1", "#86b9b0", "#62d394", "#f4c14f", "#ff9a52",
];

#[derive(Clone, PartialEq, Debug)]
pub struct Checkin {
    pub mood: u8,
    pub energy: u8,
    pub reasons: Vec<&'static str>,
    pub at: Time,
}

impl Checkin {
    pub fn summary(&self) -> String {
        format!(
            "{}, energy {}",
            MOODS[self.mood as usize],
            ENERGY[self.energy as usize].to_lowercase()
        )
    }
}

/// Suggestion chips. They follow the side of the scale he picked.
pub fn reasons(mood: u8, energy: u8) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = match mood {
        0..=2 => vec!["Tired", "Stressed", "Exams", "Phone, reels", "Lonely", "Family", "Money", "Health"],
        3 => vec!["Busy day", "Restless", "Bored", "Waiting on something", "Classes"],
        _ => vec!["Shipped something", "Good workout", "Slept well", "Good talk", "Progress at work", "Time with family"],
    };
    match energy {
        0 | 1 => out.insert(0, "Slept late"),
        4 => out.insert(0, "In the zone"),
        _ => {}
    }
    out
}
