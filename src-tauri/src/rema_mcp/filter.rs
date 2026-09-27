//! Strict filters and deterministic ranking.
//!
//! Different filter fields combine with AND, values within a field with OR;
//! exclusions win. In strict mode an unknown required value is not a match
//! (the job is "unresolved"); nothing is relaxed to fill the result count.
//! Salaries are compared only in the same currency, period and as stated
//! lower bounds; no conversions, working-hour or 12/14-payment math.

use super::{
    contract::{
        AcquisitionMode, Availability, DescriptionState, FilterMode, JobRecord, Relevance,
        SalaryBound, SearchFilters, SkillsMode, SortMode, WorkMode,
    },
    extract,
};
use crate::analytics::normalize;

#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    Match {
        notes: Vec<String>,
    },
    /// Required facts are unknown: (filter names, explanations).
    Unresolved {
        filters: Vec<String>,
        reasons: Vec<String>,
    },
    Excluded {
        reason: String,
    },
}

/// Word stems for loose matching ("engineering" ~ "engineer").
pub fn stems(text: &str) -> Vec<String> {
    const STOP: &[&str] = &[
        "and", "or", "the", "in", "for", "with", "of", "a", "an", "und", "oder", "mit", "für",
        "im", "in", "i", "w", "z", "job", "jobs", "stelle", "praca", "m", "f", "d", "x",
    ];
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty() && !STOP.contains(w))
        .map(|w| {
            let mut w = w.to_string();
            for suffix in ["ing", "er", "s"] {
                if w.len() > suffix.len() + 3 && w.ends_with(suffix) {
                    w.truncate(w.len() - suffix.len());
                }
            }
            w
        })
        .collect()
}

fn contains_word(haystack: &str, needle: &str) -> bool {
    let needle = needle.trim().to_lowercase();
    if needle.is_empty() {
        return false;
    }
    let hay = haystack.to_lowercase();
    let mut from = 0;
    while let Some(pos) = hay[from..].find(&needle).map(|p| from + p) {
        let before = hay[..pos].chars().next_back();
        let after = hay[pos + needle.len()..].chars().next();
        let boundary = |c: Option<char>| c.is_none_or(|c| !c.is_alphanumeric());
        if boundary(before) && boundary(after) {
            return true;
        }
        from = pos + needle.len();
    }
    false
}

fn same_place(a: &str, b: &str) -> bool {
    let norm = |s: &str| {
        normalize::city_name(s)
            .or_else(|| normalize::country_name(s))
            .map(str::to_string)
            .unwrap_or_else(|| s.trim().to_lowercase())
            .to_lowercase()
    };
    norm(a) == norm(b)
}

fn text_of(record: &JobRecord) -> String {
    format!(
        "{}\n{}",
        record.title,
        record.description.text.as_deref().unwrap_or_default()
    )
}

/// The query must be about this job: some significant query word appears
/// in its title (or, for search snippets, its text).
pub fn on_topic(record: &JobRecord, query: &str) -> bool {
    let wanted = stems(query);
    if wanted.is_empty() {
        return true;
    }
    let have = stems(&record.title);
    let snippet = record.description.state == DescriptionState::SnippetOnly;
    let have_text = if snippet {
        stems(&text_of(record))
    } else {
        Vec::new()
    };
    let significant: Vec<&String> = wanted.iter().filter(|w| w.len() >= 2).collect();
    let hits = significant
        .iter()
        .filter(|w| have.contains(w) || have_text.contains(w))
        .count();
    // Multi-word queries need most words; "AI Engineer" is not "Data Engineer".
    hits * 3 >= significant.len() * 2 && hits > 0
}

pub fn check(record: &JobRecord, query: &str, f: &SearchFilters, now: i64) -> Verdict {
    if matches!(
        record.quality.availability,
        Availability::Closed | Availability::Unavailable
    ) {
        return Verdict::Excluded {
            reason: "closed or no longer online".into(),
        };
    }
    let text = text_of(record);
    if let Some(term) = f.exclude_terms.iter().find(|t| contains_word(&text, t)) {
        return Verdict::Excluded {
            reason: format!("mentions the excluded term \"{term}\""),
        };
    }
    if !on_topic(record, query) {
        return Verdict::Excluded {
            reason: "not about the requested role".into(),
        };
    }
    let mut unresolved: Vec<(String, String)> = Vec::new();
    let mut notes = Vec::new();
    let mut unknown = |filter: &str, reason: String| unresolved.push((filter.to_string(), reason));

    if !f.companies.is_empty() {
        match &record.employer.name {
            None => unknown("companies", "the employer is not stated".into()),
            Some(name) => {
                let key = normalize::company_key(name);
                if !f
                    .companies
                    .iter()
                    .any(|c| normalize::company_key(c) == key || contains_word(name, c))
                {
                    return Verdict::Excluded {
                        reason: format!("employer {name} is not one of the requested ones"),
                    };
                }
            }
        }
    }
    if !f.locations.is_empty() {
        let office = f.locations.iter().any(|want| {
            record.locations.iter().any(|loc| {
                let city_ok = want.city.as_deref().is_none_or(|c| {
                    loc.city.as_deref().is_some_and(|lc| same_place(lc, c))
                        || contains_word(&loc.text, c)
                });
                let country_ok = want.country.as_deref().is_none_or(|c| {
                    loc.country.as_deref().is_some_and(|lc| same_place(lc, c))
                        || contains_word(&loc.text, c)
                });
                let region_ok = want.region.as_deref().is_none_or(|r| {
                    loc.region.as_deref().is_some_and(|lr| same_place(lr, r))
                        || contains_word(&loc.text, r)
                });
                city_ok && country_ok && region_ok
            })
        });
        let remote_ok = record.work_mode == Some(WorkMode::Remote)
            && f.locations.iter().any(|want| {
                let place = want
                    .country
                    .as_deref()
                    .or(want.city.as_deref())
                    .unwrap_or_default();
                record
                    .remote_eligibility
                    .iter()
                    .any(|e| same_place(e, place) || contains_word(e, place))
            });
        if !(office || remote_ok) {
            if record.locations.is_empty() {
                unknown("locations", "the location is not stated".into());
            } else if record.work_mode == Some(WorkMode::Remote)
                && record.remote_eligibility.is_empty()
            {
                unknown(
                    "locations",
                    "remote, but it does not say from which countries".into(),
                );
            } else {
                return Verdict::Excluded {
                    reason: "elsewhere".into(),
                };
            }
        }
    }
    if !f.work_modes.is_empty() {
        match record.work_mode {
            None => unknown("work_modes", "the work mode is not stated".into()),
            Some(mode) if !f.work_modes.contains(&mode) => {
                return Verdict::Excluded {
                    reason: "different work mode".into(),
                }
            }
            _ => {}
        }
    }
    if !f.seniority.is_empty() {
        match record.seniority {
            None => unknown("seniority", "the seniority is not stated".into()),
            Some(s) if !f.seniority.contains(&s) => {
                return Verdict::Excluded {
                    reason: "different seniority".into(),
                }
            }
            _ => {}
        }
    }
    if !f.employment_types.is_empty() {
        match record.employment_type {
            None => unknown("employment_types", "the contract type is not stated".into()),
            Some(t) if !f.employment_types.contains(&t) => {
                return Verdict::Excluded {
                    reason: "different contract type".into(),
                }
            }
            _ => {}
        }
    }
    if !f.working_time.is_empty() {
        match record.working_time {
            None => unknown("working_time", "full- or part-time is not stated".into()),
            Some(t) if !f.working_time.contains(&t) => {
                return Verdict::Excluded {
                    reason: "different working time".into(),
                }
            }
            _ => {}
        }
    }
    if f.salary_min.is_some() || f.salary_max.is_some() {
        match salary_verdict(record, f) {
            SalaryFit::Fits => {}
            SalaryFit::Unknown(reason) => unknown("salary", reason),
            SalaryFit::Below(reason) => return Verdict::Excluded { reason },
        }
    }
    if let Some(days) = f.posted_within_days {
        match record.dates.posted_at.as_deref().and_then(extract::date) {
            None => unknown(
                "posted_within_days",
                "the posting date is not stated".into(),
            ),
            Some((_, ms)) => {
                let age = normalize::days_between(ms, now);
                if age > i64::from(days) {
                    return Verdict::Excluded {
                        reason: format!("posted {age} days ago"),
                    };
                }
            }
        }
    }
    if !f.languages.is_empty() {
        match &record.language {
            None => unknown("languages", "the posting language is not known".into()),
            Some(lang) if !f.languages.iter().any(|l| l.eq_ignore_ascii_case(lang)) => {
                return Verdict::Excluded {
                    reason: format!("written in {lang}"),
                }
            }
            _ => {}
        }
    }
    if !f.required_skills.is_empty() {
        if matches!(
            record.description.state,
            DescriptionState::Missing | DescriptionState::SnippetOnly
        ) {
            unknown(
                "required_skills",
                "the full description was not available to check skills".into(),
            );
        } else {
            let found = |skill: &String| {
                contains_word(&text, skill)
                    || record
                        .description
                        .skills
                        .iter()
                        .any(|s| s.eq_ignore_ascii_case(skill))
            };
            let ok = match f.required_skills_mode {
                SkillsMode::All => f.required_skills.iter().all(found),
                SkillsMode::Any => f.required_skills.iter().any(found),
            };
            if !ok {
                return Verdict::Excluded {
                    reason: "does not mention the required skills".into(),
                };
            }
        }
    }
    if unresolved.is_empty() {
        return Verdict::Match { notes };
    }
    let (filters, reasons): (Vec<String>, Vec<String>) = unresolved.into_iter().unzip();
    match f.filter_mode {
        FilterMode::Strict => Verdict::Unresolved { filters, reasons },
        FilterMode::Lenient => {
            notes.extend(reasons.into_iter().map(|r| format!("Unverified: {r}")));
            Verdict::Match { notes }
        }
    }
}

enum SalaryFit {
    Fits,
    Unknown(String),
    Below(String),
}

fn money(v: f64, currency: &str) -> String {
    format!("{currency} {}", v.round() as i64)
}

fn salary_verdict(record: &JobRecord, f: &SearchFilters) -> SalaryFit {
    let Some(pay) = record.compensation.as_ref() else {
        return SalaryFit::Unknown("the salary is not stated".into());
    };
    if pay.estimate {
        return SalaryFit::Unknown("only an estimate is given".into());
    }
    let (Some(currency), Some(period)) = (f.salary_currency.as_deref(), f.salary_period) else {
        return SalaryFit::Unknown("no currency and period to compare with".into());
    };
    if pay.currency.as_deref() != Some(currency) || pay.period != Some(period) {
        return SalaryFit::Unknown(format!(
            "stated as {} ({}), not comparable without conversions",
            pay.text,
            pay.period
                .map(extract::period_name)
                .unwrap_or("period not stated")
        ));
    }
    if let Some(minimum) = f.salary_min {
        match (pay.bound, pay.min, pay.max) {
            (_, Some(low), _) if low >= minimum => {}
            (Some(SalaryBound::Range), Some(low), Some(high)) if high >= minimum => {
                return SalaryFit::Unknown(format!(
                    "the range starts at {}, below {}; the upper end reaches it, but a match is \
                     not confirmed",
                    money(low, currency),
                    money(minimum, currency)
                ))
            }
            (Some(SalaryBound::Floor), Some(low), _) => {
                return SalaryFit::Unknown(format!(
                    "the stated minimum is {}, below {}; the employer may pay more but does not \
                     confirm it",
                    money(low, currency),
                    money(minimum, currency)
                ))
            }
            (Some(SalaryBound::Ceiling), None, Some(_)) => {
                return SalaryFit::Unknown("only an upper bound is stated".into())
            }
            _ => {
                return SalaryFit::Below(format!(
                    "stated salary {} is below {}",
                    pay.text,
                    money(minimum, currency)
                ))
            }
        }
    }
    if let Some(maximum) = f.salary_max {
        match (pay.min, pay.max) {
            (Some(low), _) if low > maximum => {
                return SalaryFit::Below(format!("stated salary {} is above the maximum", pay.text))
            }
            (_, Some(high)) if high <= maximum => {}
            _ => return SalaryFit::Unknown("no comparable upper bound is stated".into()),
        }
    }
    SalaryFit::Fits
}

/// A deterministic ranking heuristic (0–100) and its main factors.
pub fn relevance(record: &JobRecord, query: &str, f: &SearchFilters, now: i64) -> Relevance {
    let mut score = 0.0;
    let mut factors = Vec::new();
    let wanted = stems(query);
    let have = stems(&record.title);
    if !wanted.is_empty() {
        let share = wanted.iter().filter(|w| have.contains(w)).count() as f64 / wanted.len() as f64;
        score += 40.0 * share;
        if share >= 1.0 {
            factors.push("title matches the query".to_string());
        }
    }
    if !f.skills.is_empty() {
        let text = text_of(record);
        let hits = f.skills.iter().filter(|s| contains_word(&text, s)).count();
        score += 15.0 * hits as f64 / f.skills.len() as f64;
        if hits > 0 {
            factors.push(format!("{hits} of {} preferred skills", f.skills.len()));
        }
    }
    if let Some(want) = f.locations.first() {
        let city = want.city.as_deref().is_some_and(|c| {
            record
                .locations
                .iter()
                .any(|l| l.city.as_deref().is_some_and(|lc| same_place(lc, c)))
        });
        if city {
            score += 10.0;
            factors.push("in the requested city".into());
        }
    }
    if record.work_mode.is_some_and(|m| f.work_modes.contains(&m)) {
        score += 5.0;
    }
    if let Some((_, ms)) = record.dates.posted_at.as_deref().and_then(extract::date) {
        let age = normalize::days_between(ms, now);
        let fresh = match age {
            ..=7 => 15.0,
            8..=14 => 10.0,
            15..=30 => 5.0,
            _ => 0.0,
        };
        score += fresh;
        if fresh >= 15.0 {
            factors.push("posted this week".into());
        }
    }
    let structured = record
        .evidence
        .iter()
        .any(|e| matches!(e.method.as_str(), "ats_api" | "ats_feed"));
    let json_ld = record.evidence.iter().any(|e| e.method == "json_ld");
    score += match (record.quality.acquisition, structured, json_ld) {
        (_, true, _) => 10.0,
        (AcquisitionMode::PermittedPublicPage, _, true) => 8.0,
        (AcquisitionMode::PermittedPublicPage, _, _) => 4.0,
        _ => 0.0,
    };
    if structured {
        factors.push("from the employer's ATS".into());
    }
    score += match record.description.state {
        DescriptionState::Full => 5.0,
        DescriptionState::Partial => 3.0,
        DescriptionState::SnippetOnly => 1.0,
        DescriptionState::Missing => 0.0,
    };
    Relevance {
        score: score.round().clamp(0.0, 100.0) as u32,
        factors,
    }
}

fn comparable_salary(record: &JobRecord, f: &SearchFilters) -> Option<f64> {
    let pay = record.compensation.as_ref().filter(|p| !p.estimate)?;
    if f.salary_currency
        .as_deref()
        .is_some_and(|c| pay.currency.as_deref() != Some(c))
    {
        return None;
    }
    if f.salary_period.is_some_and(|p| pay.period != Some(p)) {
        return None;
    }
    pay.min.or(pay.max)
}

/// Sorts (score, record) pairs by the requested mode. Missing values sort
/// last; the ReMa id breaks every tie.
pub fn sort(items: &mut [(Relevance, JobRecord)], mode: SortMode, f: &SearchFilters) {
    let posted = |r: &JobRecord| r.dates.posted_at.clone();
    items.sort_by(|(ra, a), (rb, b)| {
        let primary = match mode {
            SortMode::Relevance => rb.score.cmp(&ra.score),
            SortMode::Newest => match (posted(a), posted(b)) {
                (Some(x), Some(y)) => y.cmp(&x),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            },
            SortMode::SalaryAscending | SortMode::SalaryDescending => {
                match (comparable_salary(a, f), comparable_salary(b, f)) {
                    (Some(x), Some(y)) => {
                        let order = x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal);
                        if mode == SortMode::SalaryDescending {
                            order.reverse()
                        } else {
                            order
                        }
                    }
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (None, None) => std::cmp::Ordering::Equal,
                }
            }
        };
        primary
            .then_with(|| rb.score.cmp(&ra.score))
            .then_with(|| a.id.cmp(&b.id))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rema_mcp::{
        adapters::blank,
        contract::{Compensation, LocationFilter, SalaryBasis, SalaryPeriod, Seniority},
    };

    const NOW: i64 = 1_790_380_800_000; // 2026-09-26

    fn job(title: &str) -> JobRecord {
        let mut r = blank(AcquisitionMode::DocumentedPublicFeed);
        r.id = format!("rj_{}", title.len());
        r.title = title.into();
        r.locations = vec![extract::location("Vienna, Austria")];
        r.work_mode = Some(WorkMode::Hybrid);
        r.seniority = Some(Seniority::Senior);
        r.dates.posted_at = Some("2026-09-23".into());
        r.description = extract::description(
            Some("Requirements\n- Python\n- LLM experience".into()),
            false,
            true,
        );
        r
    }

    fn pay(min: Option<f64>, max: Option<f64>, bound: SalaryBound) -> Compensation {
        Compensation {
            text: "stated".into(),
            min,
            max,
            currency: Some("EUR".into()),
            period: Some(SalaryPeriod::Year),
            basis: SalaryBasis::Gross,
            bound: Some(bound),
            estimate: false,
            variable_pay_mentioned: false,
        }
    }

    fn vienna_filters() -> SearchFilters {
        SearchFilters {
            locations: vec![LocationFilter {
                city: Some("Vienna".into()),
                region: None,
                country: Some("AT".into()),
            }],
            work_modes: vec![WorkMode::Remote, WorkMode::Hybrid],
            seniority: vec![Seniority::Mid, Seniority::Senior],
            posted_within_days: Some(10),
            ..SearchFilters::default()
        }
    }

    #[test]
    fn a_confirmed_match_passes_every_filter() {
        assert_eq!(
            check(
                &job("Senior AI Engineer"),
                "AI Engineer",
                &vienna_filters(),
                NOW
            ),
            Verdict::Match { notes: vec![] }
        );
    }

    #[test]
    fn unknown_is_not_a_match_and_nothing_is_relaxed() {
        let mut filters = vienna_filters();
        filters.salary_min = Some(96_000.0);
        filters.salary_currency = Some("EUR".into());
        filters.salary_period = Some(SalaryPeriod::Year);

        // "Competitive salary": no amount.
        let competitive = job("Senior AI Engineer");
        assert!(matches!(
            check(&competitive, "AI Engineer", &filters, NOW),
            Verdict::Unresolved { ref filters, .. } if filters == &["salary"]
        ));
        // A 65,000 lower bound with possible overpayment: not confirmed.
        let mut floor = job("Senior AI Engineer");
        floor.compensation = Some(pay(Some(65_000.0), None, SalaryBound::Floor));
        match check(&floor, "AI Engineer", &filters, NOW) {
            Verdict::Unresolved { reasons, .. } => assert!(reasons[0].contains("may pay more")),
            other => panic!("{other:?}"),
        }
        // An overlapping range is not a confirmed strict match either.
        let mut overlap = job("Senior AI Engineer");
        overlap.compensation = Some(pay(Some(80_000.0), Some(110_000.0), SalaryBound::Range));
        assert!(matches!(
            check(&overlap, "AI Engineer", &filters, NOW),
            Verdict::Unresolved { .. }
        ));
        // A documented lower bound at the threshold matches.
        let mut ok = job("Senior AI Engineer");
        ok.compensation = Some(pay(Some(96_000.0), Some(120_000.0), SalaryBound::Range));
        assert!(matches!(
            check(&ok, "AI Engineer", &filters, NOW),
            Verdict::Match { .. }
        ));
        // A range entirely below is excluded.
        let mut low = job("Senior AI Engineer");
        low.compensation = Some(pay(Some(50_000.0), Some(60_000.0), SalaryBound::Range));
        assert!(matches!(
            check(&low, "AI Engineer", &filters, NOW),
            Verdict::Excluded { .. }
        ));
        // Monthly amounts are never multiplied into yearly ones.
        let mut monthly = job("Senior AI Engineer");
        let mut m = pay(Some(9_000.0), None, SalaryBound::Floor);
        m.period = Some(SalaryPeriod::Month);
        monthly.compensation = Some(m);
        match check(&monthly, "AI Engineer", &filters, NOW) {
            Verdict::Unresolved { reasons, .. } => assert!(reasons[0].contains("not comparable")),
            other => panic!("{other:?}"),
        }
        // Lenient mode keeps unknowns, with a note.
        filters.filter_mode = FilterMode::Lenient;
        match check(&competitive, "AI Engineer", &filters, NOW) {
            Verdict::Match { notes } => assert!(notes[0].starts_with("Unverified")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn dates_places_modes_and_exclusions() {
        let filters = vienna_filters();
        let mut old = job("Senior AI Engineer");
        old.dates.posted_at = Some("2026-09-01".into());
        assert!(matches!(
            check(&old, "AI Engineer", &filters, NOW),
            Verdict::Excluded { .. }
        ));
        let mut undated = job("Senior AI Engineer");
        undated.dates.posted_at = None;
        undated.dates.updated_at = Some("2026-09-25".into());
        assert!(
            matches!(
                check(&undated, "AI Engineer", &filters, NOW),
                Verdict::Unresolved { .. }
            ),
            "an update date is not a posting date"
        );
        let mut graz = job("Senior AI Engineer");
        graz.locations = vec![extract::location("Graz, Austria")];
        assert!(matches!(
            check(&graz, "AI Engineer", &filters, NOW),
            Verdict::Excluded { .. }
        ));
        let mut remote = job("Senior AI Engineer");
        remote.locations = vec![extract::location("Remote")];
        remote.work_mode = Some(WorkMode::Remote);
        assert!(
            matches!(
                check(&remote, "AI Engineer", &filters, NOW),
                Verdict::Unresolved { .. }
            ),
            "remote does not mean employable from Austria"
        );
        remote.remote_eligibility = vec!["Austria".into()];
        assert!(matches!(
            check(&remote, "AI Engineer", &filters, NOW),
            Verdict::Match { .. }
        ));
        let mut onsite = job("Senior AI Engineer");
        onsite.work_mode = Some(WorkMode::Onsite);
        assert!(matches!(
            check(&onsite, "AI Engineer", &filters, NOW),
            Verdict::Excluded { .. }
        ));
        let excluded = SearchFilters {
            exclude_terms: vec!["LLM".into()],
            ..SearchFilters::default()
        };
        assert!(matches!(
            check(&job("AI Engineer"), "AI Engineer", &excluded, NOW),
            Verdict::Excluded { .. }
        ));
        assert!(matches!(
            check(
                &job("Data Engineer"),
                "AI Engineer",
                &SearchFilters::default(),
                NOW
            ),
            Verdict::Excluded { .. }
        ));
        let mut closed = job("AI Engineer");
        closed.quality.availability = Availability::Closed;
        assert!(matches!(
            check(&closed, "AI Engineer", &SearchFilters::default(), NOW),
            Verdict::Excluded { .. }
        ));
    }

    #[test]
    fn required_skills_all_or_any() {
        let mut f = SearchFilters {
            required_skills: vec!["Python".into(), "Rust".into()],
            ..SearchFilters::default()
        };
        assert!(matches!(
            check(&job("AI Engineer"), "AI Engineer", &f, NOW),
            Verdict::Excluded { .. }
        ));
        f.required_skills_mode = SkillsMode::Any;
        assert!(matches!(
            check(&job("AI Engineer"), "AI Engineer", &f, NOW),
            Verdict::Match { .. }
        ));
    }

    #[test]
    fn sorting_is_stable_with_missing_values_last() {
        let f = SearchFilters::default();
        let mut a = job("AI Engineer");
        a.id = "rj_b".into();
        let mut b = job("AI Engineer");
        b.id = "rj_a".into();
        let mut c = job("AI Engineer");
        c.id = "rj_c".into();
        c.dates.posted_at = None;
        let mut items: Vec<(Relevance, JobRecord)> = [a, b, c]
            .into_iter()
            .map(|r| (relevance(&r, "AI Engineer", &f, NOW), r))
            .collect();
        sort(&mut items, SortMode::Newest, &f);
        let ids: Vec<&str> = items.iter().map(|(_, r)| r.id.as_str()).collect();
        assert_eq!(ids, ["rj_a", "rj_b", "rj_c"]);
    }
}
