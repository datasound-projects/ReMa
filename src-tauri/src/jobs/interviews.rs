//! Deterministic checks before an interview may reach Google Calendar.
//!
//! The model extracts; Rust verifies. A confirmed interview is written to
//! Calendar only if every critical detail is stated in the email itself:
//! the quoted date/time text must occur verbatim in the email and mention
//! the same day, month and times; the time zone must be quoted and valid;
//! the end (or duration) must be stated; start < end; it must be upcoming.
//! Anything else becomes "Needs Your Action" with an explanation.

use jiff::{civil, tz::TimeZone, ToSpan};

use super::extract::{ClaimState, InterviewClaim};
use crate::services::schedule;

/// A verified interview time and its details.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedInterview {
    pub start_at: i64,
    pub end_at: i64,
    /// IANA name for Calendar ("UTC" for fixed offsets).
    pub timezone: String,
    pub interview_type: Option<String>,
    pub location: Option<String>,
    pub meeting_url: Option<String>,
    pub participants: Vec<String>,
    /// Details that were dropped because the email did not contain them.
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InterviewCheck {
    /// Reliable and upcoming: may be synchronized to Calendar.
    Upcoming(VerifiedInterview),
    /// Reliable but already over.
    Past(VerifiedInterview),
    /// Missing, ambiguous or unverifiable details.
    NeedsReview(String),
}

/// Lowercase, unified whitespace, dashes and quotes, `a.m.` → `am`.
pub fn normalize(text: &str) -> String {
    let unified: String = text
        .chars()
        .map(|c| match c {
            '\u{2013}' | '\u{2014}' | '\u{2212}' => '-',
            '\u{2018}' | '\u{2019}' => '\'',
            '\u{201C}' | '\u{201D}' => '"',
            c if c.is_whitespace() => ' ',
            c => c,
        })
        .collect::<String>()
        .to_lowercase();
    unified
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace("a.m.", "am")
        .replace("p.m.", "pm")
}

/// Whether `needle` occurs in `haystack` not glued to other digits.
fn contains_token(haystack: &str, needle: &str) -> bool {
    let bytes = haystack.as_bytes();
    let mut from = 0;
    while let Some(pos) = haystack[from..].find(needle) {
        let start = from + pos;
        let end = start + needle.len();
        let before_ok = start == 0 || !bytes[start - 1].is_ascii_digit();
        let after_ok = end >= bytes.len() || !bytes[end].is_ascii_digit();
        if before_ok && after_ok {
            return true;
        }
        from = start + 1;
    }
    false
}

/// Whether a quote mentions a clock time in any common format.
pub fn mentions_time(quote: &str, time: civil::Time) -> bool {
    let quote = normalize(quote);
    let (h, m) = (time.hour(), time.minute());
    let h12 = if h % 12 == 0 { 12 } else { h % 12 };
    let ampm = if h < 12 { "am" } else { "pm" };
    let mut forms = vec![
        format!("{h:02}:{m:02}"),
        format!("{h}:{m:02}"),
        format!("{h:02}.{m:02}"),
        format!("{h}.{m:02}"),
        format!("{h12}:{m:02} {ampm}"),
        format!("{h12}:{m:02}{ampm}"),
        format!("{h12}.{m:02}{ampm}"),
    ];
    if m == 0 {
        forms.extend([
            format!("{h12} {ampm}"),
            format!("{h12}{ampm}"),
            format!("{h} uhr"),
            format!("{h:02} uhr"),
            format!("{h}h"),
            format!("{h:02}h"),
        ]);
    }
    forms.iter().any(|f| contains_token(&quote, f))
}

const MONTHS: [&[&str]; 12] = [
    &["january", "jan", "januar", "jänner"],
    &["february", "feb", "februar"],
    &["march", "mar", "märz", "maerz", "mär"],
    &["april", "apr"],
    &["may", "mai"],
    &["june", "jun", "juni"],
    &["july", "jul", "juli"],
    &["august", "aug"],
    &["september", "sep", "sept"],
    &["october", "oct", "oktober", "okt"],
    &["november", "nov"],
    &["december", "dec", "dezember", "dez"],
];

/// Whether a quote states this calendar date (day and month).
pub fn mentions_date(quote: &str, date: civil::Date) -> bool {
    let quote = normalize(quote);
    let (d, m) = (date.day(), date.month());
    let iso = date.to_string();
    if quote.contains(&iso) {
        return true;
    }
    let numeric = [
        format!("{d}.{m}."),
        format!("{d:02}.{m:02}"),
        format!("{d}.{m:02}"),
        format!("{d}/{m}"),
        format!("{d:02}/{m:02}"),
        format!("{m}/{d}"),
        format!("{m:02}/{d:02}"),
    ];
    if numeric.iter().any(|f| contains_token(&quote, f)) {
        return true;
    }
    let day_mentioned = [format!("{d}"), format!("{d:02}")]
        .iter()
        .any(|f| contains_token(&quote, f));
    let month_mentioned = MONTHS[(m - 1) as usize].iter().any(|name| {
        quote
            .split(|c: char| !c.is_alphanumeric())
            .any(|word| word == *name)
    });
    day_mentioned && month_mentioned
}

/// Resolves a stated time zone: IANA names and UTC/GMT offsets.
/// Returns the zone and the name to give Calendar.
pub fn resolve_timezone(value: &str) -> Option<(TimeZone, String)> {
    let value = value.trim();
    if let Ok(tz) = TimeZone::get(value) {
        if value.contains('/') || value.eq_ignore_ascii_case("utc") {
            return Some((tz, value.to_string()));
        }
    }
    let upper = value.to_ascii_uppercase().replace(' ', "");
    // Unambiguous abbreviations only (CST, IST etc. mean different zones).
    const ABBREVIATIONS: &[(&str, i32)] = &[
        ("WET", 0),
        ("WEST", 1),
        ("BST", 1),
        ("CET", 1),
        ("CEST", 2),
        ("EET", 2),
        ("EEST", 3),
        ("JST", 9),
        ("EST", -5),
        ("EDT", -4),
        ("MST", -7),
        ("MDT", -6),
        ("PST", -8),
        ("PDT", -7),
    ];
    if let Some((_, hours)) = ABBREVIATIONS.iter().find(|(name, _)| *name == upper) {
        let offset = jiff::tz::Offset::from_hours(*hours as i8).ok()?;
        return Some((TimeZone::fixed(offset), "UTC".into()));
    }
    let offset_part = upper
        .strip_prefix("UTC")
        .or_else(|| upper.strip_prefix("GMT"))
        .unwrap_or(&upper);
    if offset_part.is_empty() || offset_part == "Z" {
        return Some((TimeZone::UTC, "UTC".into()));
    }
    let sign = match offset_part.chars().next()? {
        '+' => 1,
        '-' => -1,
        _ => return None,
    };
    let digits = offset_part[1..].replace(':', "");
    let (hours, minutes) = match digits.len() {
        1 | 2 => (digits.parse::<i32>().ok()?, 0),
        4 => (
            digits[..2].parse::<i32>().ok()?,
            digits[2..].parse::<i32>().ok()?,
        ),
        _ => return None,
    };
    if hours > 14 || minutes > 59 {
        return None;
    }
    let offset = jiff::tz::Offset::from_seconds(sign * (hours * 3600 + minutes * 60)).ok()?;
    Some((TimeZone::fixed(offset), "UTC".into()))
}

fn is_http_url(url: &str) -> bool {
    reqwest::Url::parse(url).is_ok_and(|u| matches!(u.scheme(), "https" | "http"))
}

/// Checks an interview claim against the email it came from.
pub fn validate(claim: &InterviewClaim, email_text: &str, now: i64) -> InterviewCheck {
    let review = |why: &str| InterviewCheck::NeedsReview(why.to_string());
    if let Some(unclear) = &claim.unclear {
        return InterviewCheck::NeedsReview(format!("unclear interview details ({unclear})"));
    }
    if !matches!(claim.state, ClaimState::Confirmed | ClaimState::Rescheduled) {
        return review("the interview time is not confirmed yet");
    }
    let email = normalize(email_text);
    let found = |quote: &str| {
        let quote = normalize(quote);
        !quote.is_empty() && email.contains(&quote)
    };

    let (Some(date), Some(start)) = (
        claim
            .date
            .as_deref()
            .and_then(|d| schedule::parse_date(d).ok()),
        claim
            .start_time
            .as_deref()
            .and_then(|t| schedule::parse_time(t).ok()),
    ) else {
        return review("the interview date or start time is missing");
    };
    let Some(quote) = claim.datetime_quote.as_deref().filter(|q| found(q)) else {
        return review("the interview date and time could not be found in the email text");
    };
    if !mentions_date(quote, date) || !mentions_time(quote, start) {
        return review("the stated interview date or time does not match the email text");
    }

    // Never guess the time zone.
    let Some(tz_text) = claim.timezone.as_deref() else {
        return review("the email does not state the interview time zone");
    };
    if !claim.timezone_quote.as_deref().is_some_and(found) {
        return review("the interview time zone is not stated in the email text");
    }
    let Some((tz, tz_name)) = resolve_timezone(tz_text) else {
        return InterviewCheck::NeedsReview(format!("unrecognized time zone \"{tz_text}\""));
    };

    // Never guess the duration.
    let end_time = match (claim.end_time.as_deref(), claim.duration_minutes) {
        (Some(end), _) => match schedule::parse_time(end) {
            Ok(end) if mentions_time(quote, end) => end,
            _ => return review("the interview end time does not match the email text"),
        },
        (None, Some(minutes)) if (5..=480).contains(&minutes) => {
            let stated = contains_token(&normalize(email_text), &minutes.to_string());
            if !stated {
                return review("the interview duration is not stated in the email text");
            }
            match start.checked_add((minutes as i64).minutes()) {
                Ok(end) => end,
                Err(_) => {
                    return review("the interview would end after midnight; check the details")
                }
            }
        }
        _ => return review("the email does not state when the interview ends"),
    };

    let (Ok(start_at), Ok(end_at)) = (
        schedule::local_to_millis(date, start, &tz),
        schedule::local_to_millis(date, end_time, &tz),
    ) else {
        return review("the interview date is out of range");
    };
    if start_at >= end_at {
        return review("the interview end is not after its start");
    }
    if end_at - start_at > 8 * 3_600_000 {
        return review("the interview would last more than 8 hours");
    }

    let mut notes = Vec::new();
    let meeting_url = claim.meeting_url.clone().filter(|url| {
        let ok = is_http_url(url) && email_text.contains(url.as_str());
        if !ok {
            notes.push("meeting link not found in the email; left out".to_string());
        }
        ok
    });
    let location = claim.location.clone().filter(|location| {
        let ok = found(location);
        if !ok {
            notes.push("location not found in the email; left out".to_string());
        }
        ok
    });

    let verified = VerifiedInterview {
        start_at,
        end_at,
        timezone: tz_name,
        interview_type: claim.interview_type.clone(),
        location,
        meeting_url,
        participants: claim.participants.clone(),
        notes,
    };
    if start_at <= now {
        InterviewCheck::Past(verified)
    } else {
        InterviewCheck::Upcoming(verified)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EMAIL: &str = "Hi Ana,\nWe are happy to confirm your technical interview on \
        Monday, 28 September 2026 from 10:00 to 11:00 (CEST, Europe/Vienna).\n\
        Join via https://meet.example.com/abc-def\nLocation: Online\nBest, Tom";

    fn claim() -> InterviewClaim {
        InterviewClaim {
            state: ClaimState::Confirmed,
            date: Some("2026-09-28".into()),
            start_time: Some("10:00".into()),
            end_time: Some("11:00".into()),
            duration_minutes: None,
            timezone: Some("Europe/Vienna".into()),
            datetime_quote: Some("Monday, 28 September 2026 from 10:00 to 11:00".into()),
            timezone_quote: Some("CEST, Europe/Vienna".into()),
            interview_type: Some("Technical interview".into()),
            location: Some("Online".into()),
            meeting_url: Some("https://meet.example.com/abc-def".into()),
            participants: vec!["Tom".into()],
            interviewer: None,
            unclear: None,
        }
    }

    fn now() -> i64 {
        schedule::local_to_millis(
            schedule::parse_date("2026-09-25").unwrap(),
            schedule::parse_time("12:00").unwrap(),
            &TimeZone::UTC,
        )
        .unwrap()
    }

    fn reason(check: InterviewCheck) -> String {
        match check {
            InterviewCheck::NeedsReview(reason) => reason,
            other => panic!("expected review, got {other:?}"),
        }
    }

    #[test]
    fn accepts_a_fully_stated_confirmed_interview() {
        let InterviewCheck::Upcoming(v) = validate(&claim(), EMAIL, now()) else {
            panic!("expected upcoming");
        };
        assert_eq!(v.end_at - v.start_at, 3_600_000);
        assert_eq!(v.timezone, "Europe/Vienna");
        // 10:00 CEST is 08:00 UTC.
        assert_eq!(
            schedule::millis_to_local(v.start_at, &TimeZone::UTC)
                .unwrap()
                .1,
            "08:00"
        );
        assert_eq!(
            v.meeting_url.as_deref(),
            Some("https://meet.example.com/abc-def")
        );
        assert!(v.notes.is_empty());
    }

    #[test]
    fn never_guesses_the_time_zone_or_duration() {
        let mut no_tz = claim();
        no_tz.timezone = None;
        assert!(reason(validate(&no_tz, EMAIL, now())).contains("time zone"));

        let mut invented_tz = claim();
        invented_tz.timezone_quote = Some("Pacific Time".into());
        assert!(reason(validate(&invented_tz, EMAIL, now())).contains("time zone"));

        let mut no_end = claim();
        no_end.end_time = None;
        assert!(reason(validate(&no_end, EMAIL, now())).contains("ends"));

        let mut invented_end = claim();
        invented_end.end_time = Some("11:30".into());
        assert!(reason(validate(&invented_end, EMAIL, now())).contains("end time"));
    }

    #[test]
    fn rejects_dates_and_times_not_in_the_email() {
        let mut wrong_day = claim();
        wrong_day.date = Some("2026-09-29".into());
        assert!(reason(validate(&wrong_day, EMAIL, now())).contains("does not match"));

        let mut made_up_quote = claim();
        made_up_quote.datetime_quote = Some("Tuesday 29 September at 14:30".into());
        assert!(reason(validate(&made_up_quote, EMAIL, now())).contains("could not be found"));

        let mut wrong_time = claim();
        wrong_time.start_time = Some("09:00".into());
        assert!(reason(validate(&wrong_time, EMAIL, now())).contains("does not match"));
    }

    #[test]
    fn proposed_unclear_and_inverted_times_need_review() {
        let mut proposed = claim();
        proposed.state = ClaimState::Proposed;
        assert!(reason(validate(&proposed, EMAIL, now())).contains("not confirmed"));

        let mut unclear = claim();
        unclear.unclear = Some("two different times mentioned".into());
        assert!(reason(validate(&unclear, EMAIL, now())).contains("two different times"));

        let email = "Interview on 28 Sep 2026, 11:00 to 10:00 UTC";
        let mut inverted = claim();
        inverted.start_time = Some("11:00".into());
        inverted.end_time = Some("10:00".into());
        inverted.datetime_quote = Some("28 Sep 2026, 11:00 to 10:00".into());
        inverted.timezone = Some("UTC".into());
        inverted.timezone_quote = Some("UTC".into());
        inverted.meeting_url = None;
        inverted.location = None;
        assert!(reason(validate(&inverted, email, now())).contains("not after"));
    }

    #[test]
    fn drops_links_and_locations_the_email_does_not_contain() {
        let mut c = claim();
        c.meeting_url = Some("https://zoom.us/j/999".into());
        c.location = Some("Vienna HQ, 3rd floor".into());
        let InterviewCheck::Upcoming(v) = validate(&c, EMAIL, now()) else {
            panic!("expected upcoming");
        };
        assert_eq!((v.meeting_url, v.location), (None, None));
        assert_eq!(v.notes.len(), 2);
    }

    #[test]
    fn supports_durations_offsets_and_past_interviews() {
        let email = "Your call is confirmed for 3 October 2026 at 2:30 pm (UTC+2). It will take 45 minutes.";
        let c = InterviewClaim {
            date: Some("2026-10-03".into()),
            start_time: Some("14:30".into()),
            end_time: None,
            duration_minutes: Some(45),
            timezone: Some("UTC+2".into()),
            datetime_quote: Some("3 October 2026 at 2:30 pm".into()),
            timezone_quote: Some("UTC+2".into()),
            meeting_url: None,
            location: None,
            ..claim()
        };
        let InterviewCheck::Upcoming(v) = validate(&c, email, now()) else {
            panic!("expected upcoming");
        };
        assert_eq!(v.end_at - v.start_at, 45 * 60_000);
        assert_eq!(
            schedule::millis_to_local(v.start_at, &TimeZone::UTC)
                .unwrap()
                .1,
            "12:30"
        );

        let later = v.end_at + 1;
        assert!(matches!(
            validate(&c, email, later),
            InterviewCheck::Past(_)
        ));
    }

    #[test]
    fn recognizes_time_and_date_formats() {
        let t = |h, m| civil::Time::new(h, m, 0, 0).unwrap();
        assert!(mentions_time("at 2pm", t(14, 0)));
        assert!(mentions_time("um 14 Uhr", t(14, 0)));
        assert!(mentions_time("10.30 a.m.", t(10, 30)));
        assert!(
            !mentions_time("at 11:00", t(1, 0)),
            "digits must not be glued"
        );
        let d = civil::date(2026, 9, 28);
        assert!(mentions_date("28.09.2026", d));
        assert!(mentions_date("Sept 28th", d) || mentions_date("Sept 28", d));
        assert!(mentions_date("am 28. September", d));
        assert!(!mentions_date("on the 28th", d), "month required");
        assert!(resolve_timezone("+05:30").is_some());
        assert!(resolve_timezone("CEST").is_some());
        assert!(
            resolve_timezone("CST").is_none(),
            "ambiguous abbreviations need review"
        );
        assert!(resolve_timezone("Mars/Base").is_none());
    }
}
