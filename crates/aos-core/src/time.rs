//! Time helpers: timestamps, "today", and parsing the short forms people and agents type
//! ("tomorrow", "+2d", "07:30", "+15m").

use chrono::{
    DateTime, Datelike, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime, SecondsFormat,
    TimeZone, Utc, Weekday,
};

use crate::error::{Result, invalid};

/// The current instant in UTC.
pub fn now() -> DateTime<Utc> {
    Utc::now()
}

/// RFC 3339 UTC with milliseconds, e.g. `2026-11-11T05:00:00.000Z`. Sorts as text.
pub fn ts(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// Today's date on this machine's clock.
pub fn local_today() -> NaiveDate {
    Local::now().date_naive()
}

/// Parse a day: `today`, `tomorrow`, `yesterday`, `+3d`, `-1d`, `mon`..`sun` (the next one,
/// today counts), or `YYYY-MM-DD`.
pub fn parse_day(input: &str, today: NaiveDate) -> Result<NaiveDate> {
    let s = input.trim().to_ascii_lowercase();
    match s.as_str() {
        "today" | "tod" => return Ok(today),
        "tomorrow" | "tmr" | "tom" => return Ok(today + Duration::days(1)),
        "yesterday" => return Ok(today - Duration::days(1)),
        _ => {}
    }
    if let Some(n) = parse_offset(&s, 'd') {
        return Ok(today + Duration::days(n));
    }
    if let Some(n) = parse_offset(&s, 'w') {
        return Ok(today + Duration::weeks(n));
    }
    if let Some(wd) = parse_weekday(&s) {
        let ahead = (wd.num_days_from_monday() + 7 - today.weekday().num_days_from_monday()) % 7;
        return Ok(today + Duration::days(i64::from(ahead)));
    }
    NaiveDate::parse_from_str(&s, "%Y-%m-%d").map_err(|_| {
        invalid(format!(
            "can't read '{input}' as a day (try today, +2d, fri, 2026-11-20)"
        ))
    })
}

/// Parse a moment for alarms: RFC 3339, `YYYY-MM-DD HH:MM`, `HH:MM` (next time it happens),
/// `tomorrow 07:00`, or an offset `+15m`, `+2h`, `+1d`. Local forms use `now`'s time zone.
pub fn parse_when<Tz: TimeZone>(input: &str, now: &DateTime<Tz>) -> Result<DateTime<Utc>> {
    let s = input.trim();
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Ok(dt.with_timezone(&Utc));
    }
    let lower = s.to_ascii_lowercase();
    for unit in ['m', 'h', 'd'] {
        if let Some(n) = parse_offset(&lower, unit) {
            let d = match unit {
                'm' => Duration::minutes(n),
                'h' => Duration::hours(n),
                _ => Duration::days(n),
            };
            return Ok((now.clone() + d).with_timezone(&Utc));
        }
    }
    let tz = now.timezone();
    let local = |naive: NaiveDateTime| {
        tz.from_local_datetime(&naive)
            .earliest()
            .map(|dt| dt.with_timezone(&Utc))
            .ok_or_else(|| invalid(format!("'{input}' does not exist in the local time zone")))
    };
    for fmt in [
        "%Y-%m-%d %H:%M",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%dT%H:%M:%S",
    ] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(s, fmt) {
            return local(naive);
        }
    }
    // "<day> <time>", e.g. "tomorrow 07:00" or "fri 18:30".
    if let Some((day, time)) = lower.split_once(' ')
        && let Ok(t) = parse_time(time)
    {
        let d = parse_day(day, now.date_naive())?;
        return local(d.and_time(t));
    }
    // A bare time: today if still ahead, else tomorrow.
    if let Ok(t) = parse_time(&lower) {
        let today = now.date_naive();
        let candidate = local(today.and_time(t))?;
        if candidate > now.with_timezone(&Utc) {
            return Ok(candidate);
        }
        return local((today + Duration::days(1)).and_time(t));
    }
    Err(invalid(format!(
        "can't read '{input}' as a time (try 07:30, +15m, tomorrow 06:00, 2026-11-20 09:00)"
    )))
}

fn parse_time(s: &str) -> Result<NaiveTime> {
    NaiveTime::parse_from_str(s, "%H:%M")
        .or_else(|_| NaiveTime::parse_from_str(s, "%H:%M:%S"))
        .map_err(|_| invalid(format!("bad time '{s}'")))
}

/// `+3d` / `-2d` / `3d` with the given unit letter.
fn parse_offset(s: &str, unit: char) -> Option<i64> {
    let body = s.strip_suffix(unit)?;
    let (sign, digits) = match body.as_bytes().first()? {
        b'+' => (1, &body[1..]),
        b'-' => (-1, &body[1..]),
        _ => (1, body),
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse::<i64>().ok().map(|n| sign * n)
}

pub(crate) fn parse_weekday(s: &str) -> Option<Weekday> {
    Some(match s {
        "mon" | "monday" => Weekday::Mon,
        "tue" | "tues" | "tuesday" => Weekday::Tue,
        "wed" | "wednesday" => Weekday::Wed,
        "thu" | "thur" | "thurs" | "thursday" => Weekday::Thu,
        "fri" | "friday" => Weekday::Fri,
        "sat" | "saturday" => Weekday::Sat,
        "sun" | "sunday" => Weekday::Sun,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn days() {
        let today = d("2026-11-11"); // a Wednesday
        assert_eq!(parse_day("today", today).unwrap(), today);
        assert_eq!(parse_day("tomorrow", today).unwrap(), d("2026-11-12"));
        assert_eq!(parse_day("+3d", today).unwrap(), d("2026-11-14"));
        assert_eq!(parse_day("-1d", today).unwrap(), d("2026-11-10"));
        assert_eq!(parse_day("fri", today).unwrap(), d("2026-11-13"));
        assert_eq!(parse_day("wed", today).unwrap(), today);
        assert_eq!(parse_day("2026-12-01", today).unwrap(), d("2026-12-01"));
        assert!(parse_day("someday", today).is_err());
    }

    #[test]
    fn moments() {
        let ist = FixedOffset::east_opt(5 * 3600 + 1800).unwrap();
        let now = ist.with_ymd_and_hms(2026, 11, 11, 20, 0, 0).unwrap();
        let utc = |s: &str| DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc);
        assert_eq!(
            parse_when("+15m", &now).unwrap(),
            utc("2026-11-11T14:45:00Z")
        );
        // 07:00 already passed today, so it means tomorrow.
        assert_eq!(
            parse_when("07:00", &now).unwrap(),
            utc("2026-11-12T01:30:00Z")
        );
        assert_eq!(
            parse_when("21:00", &now).unwrap(),
            utc("2026-11-11T15:30:00Z")
        );
        assert_eq!(
            parse_when("tomorrow 05:00", &now).unwrap(),
            utc("2026-11-11T23:30:00Z")
        );
        assert_eq!(
            parse_when("2026-11-20 09:00", &now).unwrap(),
            utc("2026-11-20T03:30:00Z")
        );
        assert_eq!(
            parse_when("2026-11-20T09:00:00Z", &now).unwrap(),
            utc("2026-11-20T09:00:00Z")
        );
        assert!(parse_when("later", &now).is_err());
    }
}
