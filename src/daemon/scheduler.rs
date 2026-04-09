use chrono::{DateTime, Datelike, Duration as ChronoDuration, Local, NaiveDate, TimeZone, Utc};

use crate::config::{Schedule, Weekday};

#[derive(Debug, Clone, Copy)]
enum Tz {
    Named(chrono_tz::Tz),
    Local,
}

impl Tz {
    fn at_utc(self, date: NaiveDate, h: u32, m: u32) -> Option<DateTime<Utc>> {
        match self {
            Tz::Named(tz) => tz
                .with_ymd_and_hms(date.year(), date.month(), date.day(), h, m, 0)
                .single()
                .map(|dt| dt.with_timezone(&Utc)),
            Tz::Local => Local
                .with_ymd_and_hms(date.year(), date.month(), date.day(), h, m, 0)
                .single()
                .map(|dt| dt.with_timezone(&Utc)),
        }
    }
    fn today(self, now: DateTime<Utc>) -> NaiveDate {
        match self {
            Tz::Named(tz) => now.with_timezone(&tz).date_naive(),
            Tz::Local => now.with_timezone(&Local).date_naive(),
        }
    }
}

fn parse_tz(s: &str) -> Tz {
    if s == "local" {
        return Tz::Local;
    }
    match s.parse::<chrono_tz::Tz>() {
        Ok(tz) => Tz::Named(tz),
        Err(_) => {
            tracing::warn!(tz = s, "invalid tz reached scheduler (validate should have caught it); falling back to UTC");
            Tz::Named(chrono_tz::UTC)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

impl Window {
    pub fn contains(&self, now: DateTime<Utc>) -> bool {
        now >= self.start && now < self.end
    }
    pub fn remaining(&self, now: DateTime<Utc>) -> std::time::Duration {
        (self.end - now).to_std().unwrap_or_default()
    }
}

fn build_window(sch: &Schedule, date: NaiveDate) -> Option<Window> {
    let tz = parse_tz(&sch.tz);
    let (sh, sm) = Schedule::parse_hhmm(&sch.start).ok()?;
    let (eh, em) = Schedule::parse_hhmm(&sch.end).ok()?;
    let start = tz.at_utc(date, sh, sm)?;
    let end_date = if (eh, em) <= (sh, sm) { date.succ_opt()? } else { date };
    let end = tz.at_utc(end_date, eh, em)?;
    Some(Window { start, end })
}

fn day_matches(sch: &Schedule, date: NaiveDate) -> bool {
    let wd = Weekday::from_chrono(date.weekday());
    sch.days.contains(&wd)
}

pub fn current_or_next(sch: &Schedule, now: DateTime<Utc>) -> Option<Window> {
    if !sch.enabled || sch.days.is_empty() {
        return None;
    }
    let today = parse_tz(&sch.tz).today(now);
    for offset in -1..=8i64 {
        let Some(d) = today.checked_add_signed(ChronoDuration::days(offset)) else { continue };
        if !day_matches(sch, d) {
            continue;
        }
        let Some(w) = build_window(sch, d) else { continue };
        if w.end <= now {
            continue;
        }
        return Some(w);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sch(days: Vec<Weekday>, start: &str, end: &str) -> Schedule {
        Schedule {
            enabled: true,
            days,
            start: start.into(),
            end: end.into(),
            tz: "UTC".into(),
        }
    }

    #[test]
    fn in_window() {
        let s = sch(vec![Weekday::Mon, Weekday::Tue, Weekday::Wed, Weekday::Thu, Weekday::Fri], "09:00", "17:00");
        let now = Utc.with_ymd_and_hms(2026, 4, 9, 10, 0, 0).unwrap();
        let w = current_or_next(&s, now).unwrap();
        assert!(w.contains(now));
    }

    #[test]
    fn next_day() {
        let s = sch(vec![Weekday::Mon, Weekday::Tue, Weekday::Wed, Weekday::Thu, Weekday::Fri], "09:00", "17:00");
        let now = Utc.with_ymd_and_hms(2026, 4, 9, 18, 0, 0).unwrap();
        let w = current_or_next(&s, now).unwrap();
        assert_eq!(w.start, Utc.with_ymd_and_hms(2026, 4, 10, 9, 0, 0).unwrap());
    }

    #[test]
    fn cross_midnight() {
        let s = sch(vec![Weekday::Fri], "22:00", "02:00");
        let now = Utc.with_ymd_and_hms(2026, 4, 11, 0, 30, 0).unwrap();
        let w = current_or_next(&s, now).unwrap();
        assert!(w.contains(now));
    }

    #[test]
    fn disabled_returns_none() {
        let mut s = sch(vec![Weekday::Mon], "09:00", "17:00");
        s.enabled = false;
        let now = Utc.with_ymd_and_hms(2026, 4, 6, 10, 0, 0).unwrap();
        assert!(current_or_next(&s, now).is_none());
    }
}
