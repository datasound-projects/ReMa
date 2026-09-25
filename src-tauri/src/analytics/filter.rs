//! Composable deterministic filters.
//!
//! All conditions must hold (AND); the values of one condition are
//! alternatives (OR). Rules for missing data:
//!
//! - Positive conditions ("Country is Austria", "Salary ≥ €100k", "posted
//!   within 10 days") need a known value: jobs without it do not match.
//! - Negative conditions ("Company is not X", "does not require Java") keep
//!   jobs whose value is unknown, since they are not known to be excluded.
//! - Salaries are compared only in the condition's currency, yearly
//!   (monthly × 12); other currencies and periods are never converted.
//! - Posting and discovery dates are separate fields.

use super::{
    dataset::{currency_code, skill_key, Data, Job},
    matching::Evidence,
    normalize,
};
use crate::{
    error::{AppError, AppResult},
    models::analytics::{
        EmploymentType, FilterCondition, FilterField as F, FilterOp as Op, MatchState,
        RequirementKind, Seniority, WorkMode,
    },
};

const DAY_MS: i64 = 86_400_000;

pub struct Context<'a> {
    /// Midnight UTC of today.
    pub today: i64,
    pub evidence: &'a Evidence,
    /// Currency salary conditions without their own currency use.
    pub currency: Option<&'static str>,
}

fn allowed(field: F) -> &'static [Op] {
    match field {
        F::Company | F::Title | F::Role | F::Location | F::Source => &[
            Op::Is,
            Op::IsNot,
            Op::Contains,
            Op::NotContains,
            Op::Known,
            Op::Unknown,
        ],
        F::Country | F::City => &[Op::Is, Op::IsNot, Op::Known, Op::Unknown],
        F::WorkMode | F::EmploymentType | F::Seniority => {
            &[Op::Is, Op::IsNot, Op::Known, Op::Unknown]
        }
        F::Salary => &[Op::AtLeast, Op::AtMost, Op::Known, Op::Unknown],
        F::DatePosted | F::DateDiscovered => &[
            Op::WithinDays,
            Op::OlderThanDays,
            Op::OnOrAfter,
            Op::Before,
            Op::Known,
            Op::Unknown,
        ],
        F::Skill | F::Language => &[Op::HasAny, Op::HasAll, Op::HasNone],
        F::Certification => &[Op::HasAny, Op::HasNone, Op::Known, Op::Unknown],
        F::MissingSkill => &[Op::HasAny, Op::HasNone],
        F::Match | F::SkillGap => &[Op::AtLeast, Op::AtMost],
        F::SearchRun => &[Op::HasAny, Op::HasNone],
    }
}

fn date_value(text: &str) -> Option<i64> {
    let date: jiff::civil::Date = text.trim().parse().ok()?;
    Some(
        date.to_zoned(jiff::tz::TimeZone::UTC)
            .ok()?
            .timestamp()
            .as_millisecond(),
    )
}

pub fn validate(c: &FilterCondition) -> AppResult<()> {
    if !allowed(c.field).contains(&c.op) {
        return Err(AppError::validation(
            "This condition does not apply to that field.",
        ));
    }
    let needs_values = matches!(
        c.op,
        Op::Is
            | Op::IsNot
            | Op::Contains
            | Op::NotContains
            | Op::HasAny
            | Op::HasAll
            | Op::HasNone
            | Op::OnOrAfter
            | Op::Before
    );
    if needs_values && c.values.iter().all(|v| v.trim().is_empty()) {
        return Err(AppError::validation(
            "Choose at least one value for the condition.",
        ));
    }
    if c.values.len() > 50 || c.values.iter().any(|v| v.len() > 200) {
        return Err(AppError::validation(
            "The condition has too many or too long values.",
        ));
    }
    let needs_number = matches!(
        c.op,
        Op::AtLeast | Op::AtMost | Op::WithinDays | Op::OlderThanDays
    );
    if needs_number && !c.number.is_some_and(|n| n.is_finite() && n >= 0.0) {
        return Err(AppError::validation("Enter a number for the condition."));
    }
    if matches!(c.op, Op::OnOrAfter | Op::Before) && date_value(&c.values[0]).is_none() {
        return Err(AppError::validation("Enter the date as YYYY-MM-DD."));
    }
    if let Some(code) = &c.currency {
        if currency_code(code).is_none() {
            return Err(AppError::validation("Unknown currency."));
        }
    }
    Ok(())
}

fn lower(values: &[String]) -> Vec<String> {
    values
        .iter()
        .map(|v| v.trim().to_lowercase())
        .filter(|v| !v.is_empty())
        .collect()
}

/// Positive/negative text matching with the unknown-value rules.
fn text(
    op: Op,
    value: Option<&str>,
    values: &[String],
    equal: impl Fn(&str, &str) -> bool,
) -> bool {
    let wanted = lower(values);
    match (op, value) {
        (Op::Known, v) => v.is_some(),
        (Op::Unknown, v) => v.is_none(),
        (Op::Is, Some(v)) => wanted.iter().any(|w| equal(&v.to_lowercase(), w)),
        (Op::Contains, Some(v)) => wanted.iter().any(|w| v.to_lowercase().contains(w.as_str())),
        (Op::IsNot, Some(v)) => !wanted.iter().any(|w| equal(&v.to_lowercase(), w)),
        (Op::NotContains, Some(v)) => !wanted.iter().any(|w| v.to_lowercase().contains(w.as_str())),
        (Op::IsNot | Op::NotContains, None) => true,
        _ => false,
    }
}

/// Is / is-not over a set of known values (countries, cities).
fn in_set(op: Op, have: &[&str], wanted: &[String]) -> bool {
    let any = have
        .iter()
        .any(|h| wanted.iter().any(|w| h.eq_ignore_ascii_case(w)));
    match op {
        Op::Known => !have.is_empty(),
        Op::Unknown => have.is_empty(),
        Op::Is => any,
        Op::IsNot => !any,
        _ => false,
    }
}

fn enum_is(op: Op, value: Option<&str>, values: &[String]) -> bool {
    let wanted = lower(values);
    let v = value.unwrap_or("unknown");
    match op {
        Op::Known => value.is_some(),
        Op::Unknown => value.is_none(),
        Op::Is => wanted.iter().any(|w| w == v),
        Op::IsNot => !wanted.iter().any(|w| w == v),
        _ => false,
    }
}

fn number(op: Op, value: Option<f64>, threshold: f64) -> bool {
    match (op, value) {
        (Op::AtLeast, Some(v)) => v >= threshold,
        (Op::AtMost, Some(v)) => v <= threshold,
        _ => false,
    }
}

fn date(op: Op, value: Option<i64>, c: &FilterCondition, today: i64) -> bool {
    let days = c.number.unwrap_or(0.0).round() as i64;
    let at = c.values.first().and_then(|v| date_value(v));
    match (op, value) {
        (Op::Known, v) => v.is_some(),
        (Op::Unknown, v) => v.is_none(),
        (Op::WithinDays, Some(v)) => v >= today - days * DAY_MS,
        (Op::OlderThanDays, Some(v)) => v < today - days * DAY_MS,
        (Op::OnOrAfter, Some(v)) => at.is_some_and(|a| v >= a),
        (Op::Before, Some(v)) => at.is_some_and(|a| v < a),
        _ => false,
    }
}

/// Requirement conditions. A job without requirement data has unknown
/// requirements: it only passes "has none of".
fn requirements(op: Op, job: &Job, keys: &[String], kind: Option<RequirementKind>) -> bool {
    let reqs = &job.facts.requirements;
    let has = |key: &String| {
        reqs.iter()
            .any(|r| &r.key == key && kind.is_none_or(|k| r.kind == k))
    };
    match op {
        Op::HasAny => keys.iter().any(has),
        Op::HasAll => !keys.is_empty() && keys.iter().all(has),
        Op::HasNone => !keys.iter().any(has),
        Op::Known => kind.is_some_and(|k| reqs.iter().any(|r| r.kind == k)),
        Op::Unknown => kind.is_some_and(|k| !reqs.iter().any(|r| r.kind == k)),
        _ => false,
    }
}

pub fn matches(c: &FilterCondition, job: &Job, ctx: &Context) -> bool {
    let j = &job.facts.job;
    match c.field {
        F::Company => text(c.op, j.company.as_deref(), &c.values, |have, want| {
            normalize::company_key(have) == normalize::company_key(want)
        }),
        F::Title => text(c.op, Some(&j.title), &c.values, |a, b| a == b),
        F::Role => text(c.op, Some(&j.normalized_title), &c.values, |have, want| {
            have == normalize::role(want).to_lowercase()
        }),
        F::Location => text(c.op, j.location.as_deref(), &c.values, |a, b| a == b),
        F::Source => text(c.op, j.source.as_deref(), &c.values, |a, b| a == b),
        F::Country => {
            let wanted: Vec<String> = c
                .values
                .iter()
                .map(|v| {
                    normalize::country_name(v).map_or_else(|| v.trim().to_string(), str::to_string)
                })
                .collect();
            in_set(c.op, &job.facts.countries, &wanted)
        }
        F::City => {
            let wanted: Vec<String> = c
                .values
                .iter()
                .map(|v| {
                    normalize::city_name(v).map_or_else(|| v.trim().to_string(), str::to_string)
                })
                .collect();
            in_set(c.op, &job.facts.cities, &wanted)
        }
        F::WorkMode => enum_is(c.op, j.work_mode.map(WorkMode::as_str), &c.values),
        F::EmploymentType => enum_is(
            c.op,
            j.employment_type.map(EmploymentType::as_str),
            &c.values,
        ),
        F::Seniority => enum_is(c.op, j.seniority.map(Seniority::as_str), &c.values),
        F::Salary => match c.op {
            Op::Known => job.facts.salary.is_some(),
            Op::Unknown => job.facts.salary.is_none(),
            _ => {
                let currency = c
                    .currency
                    .as_deref()
                    .and_then(currency_code)
                    .or(ctx.currency);
                let value = currency.and_then(|cur| job.facts.annual_salary(cur));
                number(c.op, value, c.number.unwrap_or(0.0))
            }
        },
        F::DatePosted => date(c.op, j.date_posted, c, ctx.today),
        F::DateDiscovered => date(c.op, Some(j.date_discovered), c, ctx.today),
        F::Skill => requirements(
            c.op,
            job,
            &c.values.iter().map(|v| skill_key(v)).collect::<Vec<_>>(),
            None,
        ),
        F::Language => requirements(
            c.op,
            job,
            &c.values.iter().map(|v| skill_key(v)).collect::<Vec<_>>(),
            Some(RequirementKind::Language),
        ),
        F::Certification => requirements(
            c.op,
            job,
            &c.values.iter().map(|v| skill_key(v)).collect::<Vec<_>>(),
            Some(RequirementKind::Certification),
        ),
        F::MissingSkill => {
            if !ctx.evidence.available {
                return false;
            }
            let missing = |v: &String| job.state_of(&skill_key(v)) == Some(MatchState::Missing);
            match c.op {
                Op::HasAny => c.values.iter().any(missing),
                Op::HasNone => !c.values.iter().any(missing),
                _ => false,
            }
        }
        F::Match => number(c.op, job.coverage, c.number.unwrap_or(0.0)),
        F::SkillGap => number(c.op, job.missing().map(f64::from), c.number.unwrap_or(0.0)),
        F::SearchRun => {
            let ids: Vec<i64> = c
                .values
                .iter()
                .filter_map(|v| v.trim().parse().ok())
                .collect();
            let found = job.facts.runs.iter().any(|r| ids.contains(r));
            match c.op {
                Op::HasAny => found,
                Op::HasNone => !found,
                _ => false,
            }
        }
    }
}

fn field_name(field: F) -> &'static str {
    match field {
        F::Company => "Company",
        F::Title => "Title",
        F::Role => "Role",
        F::Location => "Location",
        F::Country => "Country",
        F::City => "City",
        F::Source => "Source",
        F::WorkMode => "Work mode",
        F::EmploymentType => "Employment",
        F::Seniority => "Seniority",
        F::Salary => "Salary",
        F::DatePosted => "Posted",
        F::DateDiscovered => "Discovered",
        F::Skill => "Requires",
        F::MissingSkill => "Missing from Profile",
        F::Match => "Profile match",
        F::SkillGap => "Missing skills",
        F::Language => "Language",
        F::Certification => "Certification",
        F::SearchRun => "Search",
    }
}

fn pretty(value: &str) -> String {
    let v = value.replace('_', " ");
    let mut chars = v.chars();
    chars.next().map_or(String::new(), |c| {
        c.to_uppercase().collect::<String>() + chars.as_str()
    })
}

/// Human description, e.g. "Work mode is Remote or Hybrid".
pub fn describe(c: &FilterCondition, data: &Data, currency: Option<&str>) -> String {
    let name = field_name(c.field);
    let values = |join: &str| -> String {
        let shown: Vec<String> = match c.field {
            F::SearchRun => c
                .values
                .iter()
                .filter_map(|v| v.parse::<i64>().ok())
                .map(|id| {
                    data.runs
                        .iter()
                        .find(|r| r.id == id)
                        .map_or(format!("#{id}"), |r| r.title.clone())
                })
                .collect(),
            F::WorkMode | F::EmploymentType | F::Seniority => {
                c.values.iter().map(|v| pretty(v)).collect()
            }
            _ => c.values.iter().map(|v| v.trim().to_string()).collect(),
        };
        shown.join(join)
    };
    let n = c.number.unwrap_or(0.0);
    match c.op {
        Op::Is => format!("{name} is {}", values(" or ")),
        Op::IsNot => format!("{name} is not {}", values(" or ")),
        Op::Contains => format!("{name} contains {}", values(" or ")),
        Op::NotContains => format!("{name} does not contain {}", values(" or ")),
        Op::HasAny => format!("{name} {}", values(" or ")),
        Op::HasAll => format!("{name} {}", values(" and ")),
        Op::HasNone => format!("{name}: none of {}", values(", ")),
        Op::Known => format!("{name} known"),
        Op::Unknown => format!("{name} unknown"),
        Op::WithinDays => format!("{name} in the last {n} days"),
        Op::OlderThanDays => format!("{name} more than {n} days ago"),
        Op::OnOrAfter => format!("{name} on or after {}", values("")),
        Op::Before => format!("{name} before {}", values("")),
        Op::AtLeast | Op::AtMost => {
            let sign = if c.op == Op::AtLeast { "≥" } else { "≤" };
            match c.field {
                F::Salary => {
                    let code = c.currency.as_deref().or(currency).unwrap_or("?");
                    format!(
                        "{name} {sign} {code} {} / year",
                        normalize::format_salary(&normalize::Salary {
                            min: Some(n),
                            max: Some(n),
                            currency: None,
                            period: None,
                        })
                        .trim_end_matches(" (currency not stated)")
                    )
                }
                F::Match => format!("{name} {sign} {n}%"),
                _ => format!("{name} {sign} {n}"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        analytics::fixtures::{facts, with_skills},
        models::analytics::SalaryPeriod,
    };

    fn cond(field: F, op: Op, values: &[&str], number: Option<f64>) -> FilterCondition {
        FilterCondition {
            field,
            op,
            values: values.iter().map(|v| v.to_string()).collect(),
            number,
            currency: None,
        }
    }

    #[test]
    fn applies_conditions_with_unknown_value_rules() {
        let evidence = Evidence::default();
        let ctx = Context {
            today: 10 * DAY_MS,
            evidence: &evidence,
            currency: Some("EUR"),
        };
        let vienna = facts(1, |j| {
            j.company = Some("Company A GmbH".into());
            j.location = Some("Vienna, Austria".into());
            j.work_mode = Some(WorkMode::Remote);
            j.salary_min = Some(90_000.0);
            j.salary_max = Some(120_000.0);
            j.salary_currency = Some("EUR".into());
            j.salary_period = Some(SalaryPeriod::Year);
            j.date_posted = Some(5 * DAY_MS);
        });
        let unknown = facts(2, |_| {});
        let dollars = facts(3, |j| {
            j.salary_min = Some(150_000.0);
            j.salary_currency = Some("USD".into());
            j.salary_period = Some(SalaryPeriod::Year);
        });
        let a = Job::evaluate(&vienna, &evidence);
        let b = Job::evaluate(&unknown, &evidence);
        let c = Job::evaluate(&dollars, &evidence);
        let check = |c: &FilterCondition, job: &Job| {
            validate(c).unwrap();
            matches(c, job, &ctx)
        };
        let country = cond(F::Country, Op::Is, &["Österreich"], None);
        assert!(check(&country, &a) && !check(&country, &b));
        let not_company = cond(F::Company, Op::IsNot, &["company a"], None);
        assert!(!check(&not_company, &a), "legal form ignored");
        assert!(
            check(&not_company, &b),
            "unknown company is kept by a negative condition"
        );
        let mode = cond(F::WorkMode, Op::Is, &["remote", "hybrid"], None);
        assert!(check(&mode, &a) && !check(&mode, &b));
        let rich = cond(F::Salary, Op::AtLeast, &[], Some(100_000.0));
        assert!(check(&rich, &a), "midpoint 105k");
        assert!(!check(&rich, &b), "no salary never passes");
        assert!(!check(&rich, &c), "USD is not compared with EUR");
        let recent = cond(F::DatePosted, Op::WithinDays, &[], Some(7.0));
        assert!(check(&recent, &a) && !check(&recent, &b));
        assert!(check(&cond(F::Salary, Op::Unknown, &[], None), &b));
    }

    #[test]
    fn requirement_conditions() {
        let evidence = Evidence::default();
        let ctx = Context {
            today: 0,
            evidence: &evidence,
            currency: None,
        };
        let job = with_skills(facts(1, |_| {}), &["Kubernetes", "Python"]);
        let none = facts(2, |_| {});
        let (a, b) = (
            Job::evaluate(&job, &evidence),
            Job::evaluate(&none, &evidence),
        );
        let k8s = cond(F::Skill, Op::HasAny, &["k8s"], None);
        assert!(matches(&k8s, &a, &ctx) && !matches(&k8s, &b, &ctx));
        assert!(matches(
            &cond(F::Skill, Op::HasAll, &["Python", "Kubernetes"], None),
            &a,
            &ctx
        ));
        assert!(!matches(
            &cond(F::Skill, Op::HasNone, &["Python"], None),
            &a,
            &ctx
        ));
        assert!(matches(
            &cond(F::Skill, Op::HasNone, &["Python"], None),
            &b,
            &ctx
        ));
        // Without a Profile nothing is "missing".
        assert!(!matches(
            &cond(F::MissingSkill, Op::HasAny, &["Python"], None),
            &a,
            &ctx
        ));
    }

    #[test]
    fn rejects_invalid_conditions() {
        assert!(validate(&cond(F::Salary, Op::Contains, &["x"], None)).is_err());
        assert!(validate(&cond(F::Country, Op::Is, &[], None)).is_err());
        assert!(validate(&cond(F::DatePosted, Op::WithinDays, &[], None)).is_err());
        assert!(validate(&cond(F::DatePosted, Op::OnOrAfter, &["20.09.2025"], None)).is_err());
        assert!(validate(&cond(F::DatePosted, Op::OnOrAfter, &["2025-09-20"], None)).is_ok());
    }
}
