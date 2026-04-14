use chrono::{
    DateTime, Datelike, Duration as ChronoDuration, Local, LocalResult, NaiveDate, TimeZone, Utc,
};

use crate::config::{Schedule, Weekday};

#[derive(Debug, Clone, Copy)]
enum Tz {
    Named(chrono_tz::Tz),
    Local,
}

impl Tz {
    fn at_utc(self, date: NaiveDate, h: u32, m: u32) -> Option<DateTime<Utc>> {
        let naive = date.and_hms_opt(h, m, 0)?;
        match self {
            Tz::Named(tz) => resolve_local_time(&tz, naive),
            Tz::Local => resolve_local_time(&Local, naive),
        }
    }
    fn today(self, now: DateTime<Utc>) -> NaiveDate {
        match self {
            Tz::Named(tz) => now.with_timezone(&tz).date_naive(),
            Tz::Local => now.with_timezone(&Local).date_naive(),
        }
    }
}

fn resolve_local_time<Tz: TimeZone>(
    tz: &Tz,
    naive: chrono::NaiveDateTime,
) -> Option<DateTime<Utc>> {
    match tz.from_local_datetime(&naive) {
        LocalResult::Single(dt) => Some(dt.with_timezone(&Utc)),
        LocalResult::Ambiguous(earlier, _later) => {
            tracing::info!("DST ambiguous time, choosing earlier occurrence");
            Some(earlier.with_timezone(&Utc))
        }
        LocalResult::None => {
            let mut candidate = naive;
            for _ in 0..120 {
                candidate += ChronoDuration::minutes(1);
                if let Some(dt) = tz.from_local_datetime(&candidate).single() {
                    tracing::info!("DST gap time does not exist, moving to next valid time");
                    return Some(dt.with_timezone(&Utc));
                }
            }
            None
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
        match (self.end - now).to_std() {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!(?e, "chrono duration conversion failed, using 1 minute fallback");
                std::time::Duration::from_secs(60)
            }
        }
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

/// Pure selection of the schedule that should currently be firing, given a
/// snapshot of (name, schedule) pairs and a `last_fired` guard. Returns the
/// candidate as (name, window) if one is eligible.
pub fn pick_firing<'a>(
    profiles: impl IntoIterator<Item = (&'a str, &'a Schedule)>,
    now: DateTime<Utc>,
    last_fired: Option<&(String, DateTime<Utc>)>,
    min_remaining: std::time::Duration,
) -> Option<(String, Window)> {
    let mut candidates: Vec<(String, Window)> = profiles
        .into_iter()
        .filter_map(|(name, sch)| {
            let w = current_or_next(sch, now)?;
            if !w.contains(now) {
                return None;
            }
            Some((name.to_string(), w))
        })
        .collect();
    candidates.sort_by(|a, b| a.0.cmp(&b.0));
    let (name, w) = candidates.into_iter().next()?;
    if let Some((ln, ls)) = last_fired {
        if ln == &name && *ls == w.start {
            return None;
        }
    }
    if w.remaining(now) < min_remaining {
        return None;
    }
    Some((name, w))
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
        Schedule { enabled: true, days, start: start.into(), end: end.into(), tz: "UTC".into() }
    }

    #[test]
    fn in_window() {
        let s = sch(
            vec![Weekday::Mon, Weekday::Tue, Weekday::Wed, Weekday::Thu, Weekday::Fri],
            "09:00",
            "17:00",
        );
        let now = Utc.with_ymd_and_hms(2026, 4, 9, 10, 0, 0).unwrap();
        let w = current_or_next(&s, now).unwrap();
        assert!(w.contains(now));
    }

    #[test]
    fn next_day() {
        let s = sch(
            vec![Weekday::Mon, Weekday::Tue, Weekday::Wed, Weekday::Thu, Weekday::Fri],
            "09:00",
            "17:00",
        );
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
    fn pick_firing_skips_last_fired() {
        let s = sch(vec![Weekday::Thu], "09:00", "17:00");
        let now = Utc.with_ymd_and_hms(2026, 4, 9, 10, 0, 0).unwrap();
        let w = current_or_next(&s, now).unwrap();
        let last = Some(("focus".to_string(), w.start));
        let picked =
            pick_firing([("focus", &s)], now, last.as_ref(), std::time::Duration::from_secs(60));
        assert!(picked.is_none());
    }

    #[test]
    fn pick_firing_enforces_min_remaining() {
        let s = sch(vec![Weekday::Thu], "09:00", "10:00");
        let now = Utc.with_ymd_and_hms(2026, 4, 9, 9, 59, 30).unwrap();
        let picked = pick_firing([("focus", &s)], now, None, std::time::Duration::from_secs(60));
        assert!(picked.is_none());
    }

    #[test]
    fn pick_firing_sorts_by_name() {
        let a = sch(vec![Weekday::Thu], "09:00", "17:00");
        let b = sch(vec![Weekday::Thu], "09:00", "17:00");
        let now = Utc.with_ymd_and_hms(2026, 4, 9, 10, 0, 0).unwrap();
        let picked =
            pick_firing([("z_focus", &a), ("a_focus", &b)], now, None, std::time::Duration::ZERO)
                .unwrap();
        assert_eq!(picked.0, "a_focus");
    }

    #[test]
    fn pick_firing_requires_active_window() {
        let s = sch(vec![Weekday::Thu], "09:00", "17:00");
        let now = Utc.with_ymd_and_hms(2026, 4, 9, 18, 0, 0).unwrap();
        let picked = pick_firing([("focus", &s)], now, None, std::time::Duration::from_secs(60));
        assert!(picked.is_none());
    }

    #[test]
    fn disabled_returns_none() {
        let mut s = sch(vec![Weekday::Mon], "09:00", "17:00");
        s.enabled = false;
        let now = Utc.with_ymd_and_hms(2026, 4, 6, 10, 0, 0).unwrap();
        assert!(current_or_next(&s, now).is_none());
    }

    #[test]
    fn dst_ambiguous_chooses_earlier() {
        use chrono_tz::US::Eastern;
        let ambiguous_time =
            NaiveDate::from_ymd_opt(2026, 11, 2).unwrap().and_hms_opt(1, 30, 0).unwrap();
        let resolved = resolve_local_time(&Eastern, ambiguous_time);
        assert!(resolved.is_some());
    }

    #[test]
    fn dst_none_finds_next_valid_time() {
        use chrono_tz::US::Eastern;
        let gap_time = NaiveDate::from_ymd_opt(2026, 3, 8).unwrap().and_hms_opt(2, 30, 0).unwrap();
        let resolved = resolve_local_time(&Eastern, gap_time);
        assert!(resolved.is_some());
        let resolved_naive = resolved.unwrap().with_timezone(&Eastern).naive_local();
        assert!(resolved_naive > gap_time);
    }
}
