//! The Today overview: everything the Today page, the morning brief and agents need about one
//! day in a single read.

use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::goals::Goal;
use crate::model::{Alarm, HabitView, Mood, TaskNode};
use crate::store::Store;
use crate::tasks::{DayScore, day_score};
use crate::xp::Level;

/// Tasks of one area for the day.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AreaTasks {
    /// `None` for tasks without an area.
    pub area_id: Option<String>,
    pub area: Option<String>,
    pub pillar_id: Option<String>,
    pub tasks: Vec<TaskNode>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TodayXp {
    pub level: Level,
    pub today: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Today {
    pub date: NaiveDate,
    pub score: DayScore,
    pub areas: Vec<AreaTasks>,
    pub habits: Vec<HabitView>,
    pub moods: Vec<Mood>,
    pub journal_written: bool,
    /// Scheduled alarms in the next 24 hours.
    pub alarms: Vec<Alarm>,
    pub xp: TodayXp,
    /// Active goals, one or more per pillar.
    pub goals: Vec<Goal>,
}

impl Store {
    pub fn today_overview(&self, day: NaiveDate, now: DateTime<Utc>) -> Result<Today> {
        let nodes = self.day_tasks(day)?;
        let score = day_score(&nodes);

        // Group by area in the areas' own order; tasks without an area go last.
        let areas_all = self.list_areas(true)?;
        let mut areas: Vec<AreaTasks> = Vec::new();
        for node in nodes {
            let key = node.task.area_id.clone();
            match areas.iter_mut().find(|a| a.area_id == key) {
                Some(group) => group.tasks.push(node),
                None => {
                    let meta = key
                        .as_ref()
                        .and_then(|k| areas_all.iter().find(|a| &a.id == k));
                    areas.push(AreaTasks {
                        area: meta.map(|a| a.name.clone()),
                        pillar_id: meta.and_then(|a| a.pillar_id.clone()),
                        area_id: key,
                        tasks: vec![node],
                    });
                }
            }
        }
        let rank = |id: &Option<String>| {
            id.as_ref()
                .and_then(|k| areas_all.iter().position(|a| &a.id == k))
                .unwrap_or(usize::MAX)
        };
        areas.sort_by_key(|a| rank(&a.area_id));

        let horizon = now + Duration::hours(24);
        let alarms = self
            .list_alarms(now, false)?
            .into_iter()
            .filter(|a| {
                DateTime::parse_from_rfc3339(&a.at).is_ok_and(|t| t.with_timezone(&Utc) <= horizon)
            })
            .collect();

        let xp = self.xp_summary(day)?;
        Ok(Today {
            date: day,
            score,
            areas,
            habits: self.list_habits(day)?,
            moods: self.moods_on(day)?,
            journal_written: self.journal_written_on(day)?,
            alarms,
            xp: TodayXp {
                level: xp.overall,
                today: xp.today,
            },
            goals: self.list_goals(false)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::AlarmKind;
    use crate::tasks::NewTask;

    #[test]
    fn overview_groups_by_area() {
        let store = Store::open_temporary().unwrap();
        let day = NaiveDate::from_ymd_opt(2026, 11, 11).unwrap();
        let now = crate::time::now();
        let add = |title: &str, area: Option<&str>| {
            store
                .add_task(
                    "t",
                    NewTask {
                        title: title.into(),
                        area_id: area.map(String::from),
                        due: Some(day),
                        ..Default::default()
                    },
                )
                .unwrap()
        };
        add("Read 20 pages", Some("reading"));
        let gym = add("Gym", Some("fitness"));
        add("Loose task", None);
        store.complete_task("t", &gym.id, None, day).unwrap();
        store
            .set_alarm(
                "t",
                now + Duration::hours(1),
                "Wind down",
                AlarmKind::Reminder,
                None,
            )
            .unwrap();

        let t = store.today_overview(day, now).unwrap();
        let order: Vec<Option<&str>> = t.areas.iter().map(|a| a.area_id.as_deref()).collect();
        assert_eq!(order, vec![Some("fitness"), Some("reading"), None]);
        assert_eq!((t.score.done, t.score.counted), (1, 3));
        assert_eq!(t.xp.today, 10);
        assert_eq!(t.alarms.len(), 1);
        assert!(!t.journal_written);
    }
}
