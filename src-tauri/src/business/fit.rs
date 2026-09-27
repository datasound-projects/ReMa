//! Reproducible fit assessment (B10). The model never invents a chance
//! that a company will buy: ReMa evaluates hard constraints (pass, fail,
//! unknown), then computes an interpretable fit measure from validated
//! evidence with a versioned policy:
//!
//! ```text
//! use-case/problem alignment       0.35
//! technical/delivery compatibility 0.25
//! industry/workflow relevance      0.20
//! commercial-scale suitability     0.20
//!
//! coverage  = sum of the (renormalized) weights of criteria with a known value
//! fit score = 100 × Σ(weight × match) / coverage     (null when coverage is 0)
//! ```
//!
//! These are engineering defaults, not validated conversion weights. A
//! numeric score is shown only at coverage ≥ 0.60 ("Insufficient
//! evidence" below); unknown is never zero.

use std::cmp::Ordering;

use super::model::{ClientProspect, CriterionScore, HardCheck, HardResult};

/// The policy's version, stored with every assessment.
pub const SCORING_POLICY: &str = "business-fit-2026-09-27";
/// Below this coverage the numeric score is withheld.
pub const COVERAGE_THRESHOLD: f64 = 0.60;

/// (id, label, weight): non-negative, summing to one.
pub const CRITERIA: [(&str, &str, f64); 4] = [
    ("use_case", "Use-case / problem alignment", 0.35),
    ("technical", "Technical / delivery compatibility", 0.25),
    ("industry", "Industry / workflow relevance", 0.20),
    ("scale", "Commercial-scale suitability", 0.20),
];

/// One criterion's evaluation before weighting.
#[derive(Debug, Clone, PartialEq)]
pub struct Evaluation {
    pub applicable: bool,
    pub value: Option<f64>,
    pub reason: String,
    pub evidence: Vec<u32>,
}

impl Evaluation {
    pub fn inapplicable(reason: &str) -> Self {
        Self {
            applicable: false,
            value: None,
            reason: reason.into(),
            evidence: Vec::new(),
        }
    }

    pub fn unknown(reason: &str) -> Self {
        Self {
            applicable: true,
            value: None,
            reason: reason.into(),
            evidence: Vec::new(),
        }
    }

    pub fn known(value: f64, reason: &str, evidence: Vec<u32>) -> Self {
        Self {
            applicable: true,
            value: Some(value.clamp(0.0, 1.0)),
            reason: reason.into(),
            evidence,
        }
    }
}

/// The weighted criteria: inapplicable ones get weight 0 and the rest are
/// renormalized to sum to one.
pub fn weigh(evaluations: [Evaluation; 4]) -> Vec<CriterionScore> {
    let applicable: f64 = CRITERIA
        .iter()
        .zip(&evaluations)
        .filter(|(_, e)| e.applicable)
        .map(|((_, _, w), _)| *w)
        .sum();
    CRITERIA
        .iter()
        .zip(evaluations)
        .map(|((id, label, w), e)| CriterionScore {
            id: (*id).to_string(),
            label: (*label).to_string(),
            weight: if e.applicable && applicable > 0.0 {
                w / applicable
            } else {
                0.0
            },
            applicable: e.applicable,
            value: if e.applicable { e.value } else { None },
            reason: e.reason,
            evidence: e.evidence,
        })
        .collect()
}

fn round(x: f64) -> f64 {
    (x * 10_000.0).round() / 10_000.0
}

/// (coverage, score, whether the score may be shown).
pub fn score(criteria: &[CriterionScore]) -> (f64, Option<f64>, bool) {
    let known: Vec<&CriterionScore> = criteria
        .iter()
        .filter(|c| c.applicable && c.value.is_some())
        .collect();
    let coverage: f64 = known.iter().map(|c| c.weight).sum();
    let coverage = round(coverage.min(1.0));
    if coverage <= 0.0 {
        return (0.0, None, false);
    }
    let weighted: f64 = known
        .iter()
        .map(|c| c.weight * c.value.unwrap_or(0.0))
        .sum();
    let score = round(100.0 * weighted / coverage);
    (coverage, Some(score), coverage >= COVERAGE_THRESHOLD)
}

/// Where a candidate belongs after its hard constraints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Confirmed,
    NeedsVerification,
    Excluded,
}

pub fn group(hard: &[HardCheck]) -> Group {
    if hard.iter().any(|h| h.result == HardResult::Fail) {
        Group::Excluded
    } else if hard.iter().any(|h| h.result == HardResult::Unknown) {
        Group::NeedsVerification
    } else {
        Group::Confirmed
    }
}

/// Best first within a group: a shown score, then the score, coverage,
/// the most recent relevant signal and a stable key — an almost
/// unresearched company never outranks researched ones on one perfect
/// attribute.
pub fn compare(
    a: &ClientProspect,
    b: &ClientProspect,
    recency: impl Fn(&ClientProspect) -> i64,
) -> Ordering {
    let shown = |p: &ClientProspect| p.assessment.score_shown;
    let value = |p: &ClientProspect| {
        if p.assessment.score_shown {
            p.assessment.score.unwrap_or(0.0)
        } else {
            0.0
        }
    };
    shown(b)
        .cmp(&shown(a))
        .then(value(b).partial_cmp(&value(a)).unwrap_or(Ordering::Equal))
        .then(
            b.assessment
                .coverage
                .partial_cmp(&a.assessment.coverage)
                .unwrap_or(Ordering::Equal),
        )
        .then(recency(b).cmp(&recency(a)))
        .then(a.company_key.cmp(&b.company_key))
}

/// "72 (evidence coverage 80%)", "Insufficient evidence (coverage 35%)".
pub fn label(score: Option<f64>, coverage: f64, shown: bool) -> String {
    let coverage = format!("{:.0}%", coverage * 100.0);
    match (score, shown) {
        (Some(s), true) => format!("{s:.0} (evidence coverage {coverage})"),
        _ => format!("Insufficient evidence (coverage {coverage})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weights_sum_to_one() {
        let sum: f64 = CRITERIA.iter().map(|(_, _, w)| w).sum();
        assert!((sum - 1.0).abs() < 1e-9);
    }

    #[test]
    fn unknown_lowers_coverage_not_the_score() {
        let criteria = weigh([
            Evaluation::known(1.0, "use case named on the site", vec![0]),
            Evaluation::unknown("no technical evidence found"),
            Evaluation::known(0.5, "adjacent industry", vec![1]),
            Evaluation::unknown("size unknown"),
        ]);
        let (coverage, score, shown) = score(&criteria);
        assert!((coverage - 0.55).abs() < 1e-9, "{coverage}");
        // (0.35×1 + 0.20×0.5) / 0.55 = 81.82
        assert!((score.unwrap() - 81.8182).abs() < 1e-3, "{score:?}");
        assert!(!shown, "below 0.60 coverage the number is withheld");
        assert_eq!(
            label(score, coverage, shown),
            "Insufficient evidence (coverage 55%)"
        );
    }

    #[test]
    fn zero_coverage_is_null_without_dividing_by_zero() {
        let criteria = weigh([
            Evaluation::unknown("a"),
            Evaluation::unknown("b"),
            Evaluation::unknown("c"),
            Evaluation::unknown("d"),
        ]);
        assert_eq!(score(&criteria), (0.0, None, false));
        let none = weigh([
            Evaluation::inapplicable("a"),
            Evaluation::inapplicable("b"),
            Evaluation::inapplicable("c"),
            Evaluation::inapplicable("d"),
        ]);
        assert_eq!(score(&none), (0.0, None, false));
    }

    #[test]
    fn inapplicable_criteria_renormalize_the_rest() {
        let criteria = weigh([
            Evaluation::known(1.0, "x", vec![]),
            Evaluation::inapplicable("the offer states no technical requirements"),
            Evaluation::known(0.0, "industry known and different", vec![]),
            Evaluation::known(1.0, "size in range", vec![]),
        ]);
        assert_eq!(criteria[1].weight, 0.0);
        // 0.35/0.75, 0.20/0.75, 0.20/0.75
        let (coverage, score, shown) = score(&criteria);
        assert!((coverage - 1.0).abs() < 1e-3, "{coverage}");
        assert!((score.unwrap() - 73.33).abs() < 0.01, "{score:?}");
        assert!(shown);
        // Reproducible: the same stored criteria give the same score.
        let stored = serde_json::to_string(&criteria).unwrap();
        let again: Vec<CriterionScore> = serde_json::from_str(&stored).unwrap();
        assert_eq!(super::score(&again), (coverage, score, shown));
    }

    #[test]
    fn a_confirmed_hard_failure_excludes_and_unknown_needs_verification() {
        let check = |result| HardCheck {
            name: "location".into(),
            result,
            detail: String::new(),
        };
        assert_eq!(group(&[check(HardResult::Pass)]), Group::Confirmed);
        assert_eq!(
            group(&[check(HardResult::Pass), check(HardResult::Unknown)]),
            Group::NeedsVerification
        );
        assert_eq!(
            group(&[check(HardResult::Unknown), check(HardResult::Fail)]),
            Group::Excluded
        );
    }
}
