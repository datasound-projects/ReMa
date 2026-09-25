//! Deterministic schedule calculation. Pure functions: no I/O, no clock.
//!
//! A task's occurrences are derived from its `Schedule`, its start instant
//! and its IANA timezone. Daily and weekly schedules follow wall-clock time
//! in that timezone (08:00 stays 08:00 across daylight-saving changes);
//! interval schedules are exact durations from the start.

use jiff::{
    civil::{Date, DateTime, Time},
    tz::TimeZone,
    Timestamp, ToSpan,
};

use crate::{
    error::{AppError, AppResult},
    models::task::{IntervalUnit, Schedule, Weekday},
};

/// No task may run more often than this.
pub const MIN_INTERVAL_MINUTES: u32 = 15;
const MAX_EVERY_DAYS: u32 = 365;

pub fn validate(schedule: &Schedule) -> AppResult<()> {
    match schedule {
        Schedule::Once => Ok(()),
        Schedule::Interval { every, unit } => {
            let minutes = interval_minutes(*every, *unit);
            if minutes < u64::from(MIN_INTERVAL_MINUTES) {
                Err(AppError::validation(format!(
                    "Tasks can run at most once every {MIN_INTERVAL_MINUTES} minutes."
                )))
            } else if minutes > 60 * 24 * 365 {
                Err(AppError::validation("The interval is too long."))
            } else {
                Ok(())
            }
        }
        Schedule::Daily { every } if *every == 0 || *every > MAX_EVERY_DAYS => Err(
            AppError::validation(format!("Repeat every 1 to {MAX_EVERY_DAYS} days.")),
        ),
        Schedule::Daily { .. } => Ok(()),
        Schedule::Weekly { days } if days.is_empty() => {
            Err(AppError::validation("Choose at least one weekday."))
        }
        Schedule::Weekly { .. } => Ok(()),
    }
}

fn interval_minutes(every: u32, unit: IntervalUnit) -> u64 {
    match unit {
        IntervalUnit::Minutes => u64::from(every),
        IntervalUnit::Hours => u64::from(every) * 60,
    }
}

pub fn timezone(name: &str) -> AppResult<TimeZone> {
    TimeZone::get(name).map_err(|_| AppError::validation(format!("Unknown timezone \"{name}\".")))
}

/// The system timezone's IANA name (falls back to UTC).
pub fn system_timezone_name() -> String {
    TimeZone::system()
        .iana_name()
        .map(str::to_string)
        .unwrap_or_else(|| "UTC".into())
}

pub fn parse_date(value: &str) -> AppResult<Date> {
    value
        .trim()
        .parse::<Date>()
        .map_err(|_| AppError::validation("Enter a valid date."))
}

pub fn parse_time(value: &str) -> AppResult<Time> {
    let value = value.trim();
    let (hour, minute) = value
        .split_once(':')
        .and_then(|(h, m)| Some((h.parse::<i8>().ok()?, m.get(..2)?.parse::<i8>().ok()?)))
        .ok_or_else(|| AppError::validation("Enter a valid time (HH:MM)."))?;
    Time::new(hour, minute, 0, 0).map_err(|_| AppError::validation("Enter a valid time (HH:MM)."))
}

/// Local date + time in `tz` → epoch milliseconds. Times skipped by a DST
/// change resolve to the later valid time.
pub fn local_to_millis(date: Date, time: Time, tz: &TimeZone) -> AppResult<i64> {
    to_millis(date.to_datetime(time), tz)
}

fn to_millis(datetime: DateTime, tz: &TimeZone) -> AppResult<i64> {
    datetime
        .to_zoned(tz.clone())
        .map(|z| z.timestamp().as_millisecond())
        .map_err(|_| AppError::validation("That date is out of range."))
}

/// The last millisecond of `date` in `tz` (end-of-day end conditions).
pub fn end_of_day_millis(date: Date, tz: &TimeZone) -> AppResult<i64> {
    let next = date
        .checked_add(1.day())
        .map_err(|_| AppError::validation("That date is out of range."))?;
    Ok(to_millis(next.to_datetime(Time::midnight()), tz)? - 1)
}

fn zoned(millis: i64, tz: &TimeZone) -> AppResult<jiff::Zoned> {
    Timestamp::from_millisecond(millis)
        .map(|t| t.to_zoned(tz.clone()))
        .map_err(|_| AppError::internal("timestamp out of range"))
}

/// Epoch milliseconds → (`YYYY-MM-DD`, `HH:MM`) in `tz`.
pub fn millis_to_local(millis: i64, tz: &TimeZone) -> AppResult<(String, String)> {
    let z = zoned(millis, tz)?;
    Ok((z.date().to_string(), z.strftime("%H:%M").to_string()))
}

/// The first occurrence at or after `start_at` that is strictly later than
/// `after` (if given). `None` when the schedule has no such occurrence.
pub fn next_occurrence(
    schedule: &Schedule,
    tz: &TimeZone,
    start_at: i64,
    after: Option<i64>,
) -> AppResult<Option<i64>> {
    let is_candidate = |t: i64| t >= start_at && after.is_none_or(|a| t > a);
    match schedule {
        Schedule::Once => Ok(Some(start_at).filter(|t| is_candidate(*t))),

        Schedule::Interval { every, unit } => {
            let step = interval_minutes(*every, *unit) as i64 * 60_000;
            if step <= 0 {
                return Ok(None);
            }
            let k = match after {
                Some(a) if a >= start_at => (a - start_at) / step + 1,
                _ => 0,
            };
            Ok(start_at.checked_add(k * step))
        }

        Schedule::Daily { every } => {
            let every = i64::from((*every).max(1));
            let start = zoned(start_at, tz)?;
            let (first_date, time) = (start.date(), start.time());
            // Jump close to `after`, then step forward.
            let mut k = match after {
                Some(a) if a > start_at => {
                    let days = first_date
                        .until(zoned(a, tz)?.date())
                        .map_err(|_| AppError::internal("date arithmetic failed"))?
                        .get_days() as i64;
                    (days / every - 1).max(0)
                }
                _ => 0,
            };
            for _ in 0..1_000 {
                let date = first_date
                    .checked_add((k * every).days())
                    .map_err(|_| AppError::internal("date out of range"))?;
                let t = local_to_millis(date, time, tz)?;
                if is_candidate(t) {
                    return Ok(Some(t));
                }
                k += 1;
            }
            Ok(None)
        }

        Schedule::Weekly { days } => {
            if days.is_empty() {
                return Ok(None);
            }
            let start = zoned(start_at, tz)?;
            let time = start.time();
            let mut date = match after {
                Some(a) if a > start_at => zoned(a, tz)?.date(),
                _ => start.date(),
            };
            for _ in 0..15 {
                if days.contains(&weekday(date)) {
                    let t = local_to_millis(date, time, tz)?;
                    if is_candidate(t) {
                        return Ok(Some(t));
                    }
                }
                date = date
                    .checked_add(1.day())
                    .map_err(|_| AppError::internal("date out of range"))?;
            }
            Ok(None)
        }
    }
}

fn weekday(date: Date) -> Weekday {
    match date.weekday() {
        jiff::civil::Weekday::Monday => Weekday::Mon,
        jiff::civil::Weekday::Tuesday => Weekday::Tue,
        jiff::civil::Weekday::Wednesday => Weekday::Wed,
        jiff::civil::Weekday::Thursday => Weekday::Thu,
        jiff::civil::Weekday::Friday => Weekday::Fri,
        jiff::civil::Weekday::Saturday => Weekday::Sat,
        jiff::civil::Weekday::Sunday => Weekday::Sun,
    }
}

/// Limits that end a schedule.
#[derive(Debug, Clone, Copy, Default)]
pub struct Limits {
    pub end_at: Option<i64>,
    pub max_runs: Option<u32>,
}

/// The next run after `after`, honouring the end date and run limit.
/// `runs_so_far` counts scheduled runs already started.
pub fn next_run(
    schedule: &Schedule,
    tz: &TimeZone,
    start_at: i64,
    limits: Limits,
    runs_so_far: u32,
    after: Option<i64>,
) -> AppResult<Option<i64>> {
    let run_limit_reached = limits.max_runs.is_some_and(|max| runs_so_far >= max)
        || (matches!(schedule, Schedule::Once) && runs_so_far >= 1);
    if run_limit_reached {
        return Ok(None);
    }
    Ok(next_occurrence(schedule, tz, start_at, after)?
        .filter(|t| limits.end_at.is_none_or(|end| *t <= end)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vienna() -> TimeZone {
        timezone("Europe/Vienna").unwrap()
    }

    fn at(date: &str, time: &str) -> i64 {
        local_to_millis(
            parse_date(date).unwrap(),
            parse_time(time).unwrap(),
            &vienna(),
        )
        .unwrap()
    }

    fn local(millis: i64) -> String {
        let (date, time) = millis_to_local(millis, &vienna()).unwrap();
        format!("{date} {time}")
    }

    /// The next `n` occurrences after `from`.
    fn upcoming(schedule: &Schedule, start: i64, from: i64, n: usize) -> Vec<String> {
        let mut after = Some(from);
        let mut out = Vec::new();
        for _ in 0..n {
            let Some(t) = next_occurrence(schedule, &vienna(), start, after).unwrap() else {
                break;
            };
            out.push(local(t));
            after = Some(t);
        }
        out
    }

    #[test]
    fn enforces_the_minimum_interval() {
        let every = |every, unit| Schedule::Interval { every, unit };
        assert!(validate(&every(14, IntervalUnit::Minutes)).is_err());
        assert!(validate(&every(0, IntervalUnit::Hours)).is_err());
        assert!(validate(&every(15, IntervalUnit::Minutes)).is_ok());
        assert!(validate(&every(1, IntervalUnit::Hours)).is_ok());
        assert!(validate(&Schedule::Daily { every: 0 }).is_err());
        assert!(validate(&Schedule::Weekly { days: vec![] }).is_err());
    }

    #[test]
    fn one_time_runs_once_at_the_start() {
        let start = at("2026-09-25", "18:00");
        assert_eq!(
            next_occurrence(&Schedule::Once, &vienna(), start, None).unwrap(),
            Some(start)
        );
        assert_eq!(
            next_occurrence(&Schedule::Once, &vienna(), start, Some(start)).unwrap(),
            None
        );
        let limits = Limits::default();
        assert_eq!(
            next_run(&Schedule::Once, &vienna(), start, limits, 1, None).unwrap(),
            None
        );
    }

    #[test]
    fn intervals_are_exact_durations_from_the_start() {
        let schedule = Schedule::Interval {
            every: 4,
            unit: IntervalUnit::Hours,
        };
        let start = at("2026-09-26", "08:00");
        assert_eq!(
            upcoming(&schedule, start, start - 1, 3),
            ["2026-09-26 08:00", "2026-09-26 12:00", "2026-09-26 16:00"]
        );
        // Missed runs are skipped: the next run is the first one after `after`.
        let later = at("2026-09-27", "09:30");
        assert_eq!(upcoming(&schedule, start, later, 1), ["2026-09-27 12:00"]);
    }

    #[test]
    fn daily_keeps_wall_clock_time_across_dst() {
        let schedule = Schedule::Daily { every: 1 };
        // Vienna leaves daylight saving time on 2026-10-25.
        let start = at("2026-10-24", "08:00");
        assert_eq!(
            upcoming(&schedule, start, start - 1, 3),
            ["2026-10-24 08:00", "2026-10-25 08:00", "2026-10-26 08:00"]
        );
    }

    #[test]
    fn every_n_days_counts_from_the_start_date() {
        let schedule = Schedule::Daily { every: 3 };
        let start = at("2026-09-26", "18:00");
        let after = at("2026-10-01", "12:00");
        assert_eq!(
            upcoming(&schedule, start, after, 2),
            ["2026-10-02 18:00", "2026-10-05 18:00"]
        );
    }

    #[test]
    fn weekly_runs_on_selected_weekdays() {
        // 2026-09-26 is a Saturday.
        let weekdays = Schedule::Weekly {
            days: vec![
                Weekday::Mon,
                Weekday::Tue,
                Weekday::Wed,
                Weekday::Thu,
                Weekday::Fri,
            ],
        };
        let start = at("2026-09-26", "09:00");
        assert_eq!(
            upcoming(&weekdays, start, start - 1, 3),
            ["2026-09-28 09:00", "2026-09-29 09:00", "2026-09-30 09:00"]
        );

        let mondays = Schedule::Weekly {
            days: vec![Weekday::Mon],
        };
        assert_eq!(
            upcoming(&mondays, start, at("2026-09-28", "08:59"), 2),
            ["2026-09-28 09:00", "2026-10-05 09:00"]
        );
    }

    #[test]
    fn starts_in_the_future_are_respected() {
        let schedule = Schedule::Daily { every: 1 };
        let start = at("2026-12-01", "07:30");
        let now = at("2026-09-25", "10:00");
        assert_eq!(upcoming(&schedule, start, now, 1), ["2026-12-01 07:30"]);
    }

    #[test]
    fn end_date_and_run_limits_stop_the_schedule() {
        let schedule = Schedule::Interval {
            every: 12,
            unit: IntervalUnit::Hours,
        };
        let start = at("2026-09-26", "08:00");
        let tz = vienna();
        let end = end_of_day_millis(parse_date("2026-09-27").unwrap(), &tz).unwrap();
        let limits = Limits {
            end_at: Some(end),
            max_runs: None,
        };
        assert_eq!(
            next_run(
                &schedule,
                &tz,
                start,
                limits,
                3,
                Some(at("2026-09-27", "08:00"))
            )
            .unwrap()
            .map(local),
            Some("2026-09-27 20:00".into())
        );
        assert_eq!(
            next_run(
                &schedule,
                &tz,
                start,
                limits,
                4,
                Some(at("2026-09-27", "20:00"))
            )
            .unwrap(),
            None
        );

        let limited = Limits {
            end_at: None,
            max_runs: Some(2),
        };
        assert!(next_run(&schedule, &tz, start, limited, 1, Some(start))
            .unwrap()
            .is_some());
        assert_eq!(
            next_run(&schedule, &tz, start, limited, 2, Some(start)).unwrap(),
            None
        );
    }

    #[test]
    fn parses_and_formats_local_times() {
        assert!(parse_time("25:00").is_err());
        assert!(parse_time("8").is_err());
        assert_eq!(parse_time("08:05").unwrap(), Time::new(8, 5, 0, 0).unwrap());
        assert!(timezone("Mars/Olympus").is_err());
        assert_eq!(local(at("2026-09-25", "18:00")), "2026-09-25 18:00");
    }
}
