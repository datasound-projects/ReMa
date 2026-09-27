//! The first filter, before any model sees anything: cheap, deterministic
//! signals from a message's headers and snippet.
//!
//! - **Strong** messages (a known application thread, a known recruiter or
//!   company domain, an applicant-tracking system) go straight to
//!   classification.
//! - **Candidates** (job words, reference numbers, a known company or role
//!   named) get a headers-only relevance check by the model first.
//! - Everything else is **filtered**: never sent to a model, and only its
//!   id, thread, date and sender domain are kept.

use std::collections::HashSet;

use crate::{connectors::mail::MailMessage, jobs::applications};

/// Score at or above which a message goes straight to classification.
pub const STRONG: i64 = 60;
/// Score at or above which a message gets the headers-only check.
pub const CANDIDATE: i64 = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Strong,
    Candidate,
    Filtered,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scored {
    pub verdict: Verdict,
    pub score: i64,
    pub reasons: Vec<&'static str>,
}

/// What ReMa already knows about the user's applications.
#[derive(Debug, Clone, Default)]
pub struct Signals {
    pub known_threads: HashSet<String>,
    /// Company email domains of applications (not shared ATS/mail domains).
    pub known_domains: HashSet<String>,
    /// Normalized company names (`company_key`) of applications.
    pub known_companies: HashSet<String>,
    /// Normalized role names (`role_key`) of applications.
    pub known_roles: HashSet<String>,
    /// Addresses that sent job-related mail before.
    pub known_senders: HashSet<String>,
}

/// Applicant-tracking and recruiting platforms: mail from them is almost
/// always about an application.
const ATS_DOMAINS: &[&str] = &[
    "greenhouse.io",
    "greenhouse-mail.io",
    "lever.co",
    "hire.lever.co",
    "myworkday.com",
    "workday.com",
    "smartrecruiters.com",
    "recruitee.com",
    "personio.de",
    "personio.com",
    "successfactors.com",
    "successfactors.eu",
    "icims.com",
    "workable.com",
    "workablemail.com",
    "bamboohr.com",
    "teamtailor.com",
    "teamtailor-mail.com",
    "join.com",
    "ashbyhq.com",
    "softgarden.io",
    "softgarden.de",
    "jobvite.com",
    "taleo.net",
    "breezy.hr",
    "jazzhr.com",
    "pinpointhq.com",
    "rippling-ats.com",
];

/// Application words in a subject (English, German).
const SUBJECT_WORDS: &[&str] = &[
    "application",
    "applied",
    "your candidacy",
    "interview",
    "next steps",
    "recruiter",
    "recruiting",
    "hiring",
    "position",
    "assessment",
    "coding challenge",
    "take-home",
    "offer",
    "thank you for applying",
    "thanks for applying",
    "unfortunately",
    "bewerbung",
    "vorstellungsgespräch",
    "kennenlernen",
    "absage",
    "zusage",
    "einladung",
    "stelle",
];

/// Phrases of job alerts, newsletters and marketing.
const BULK_PHRASES: &[&str] = &[
    "job alert",
    "jobs for you",
    "recommended jobs",
    "new jobs",
    "jobs you may be interested",
    "jobs matching",
    "newsletter",
    "webinar",
    "% off",
    "sale ends",
    "stellenangebote für sie",
    "jobagent",
    "neue jobs",
];

const BULK_SENDERS: &[&str] = &[
    "jobalerts-noreply@linkedin.com",
    "jobs-noreply@linkedin.com",
    "noreply@glassdoor.com",
    "alert@indeed.com",
    "newsletter",
    "marketing",
];

fn domain_matches(domain: &str, list: &[&str]) -> bool {
    list.iter()
        .any(|d| domain == *d || domain.ends_with(&format!(".{d}")))
}

fn has_word(text: &str, word: &str) -> bool {
    // Word-ish match: "position" but not "composition".
    text.match_indices(word).any(|(i, _)| {
        let before = text[..i].chars().next_back();
        let after = text[i + word.len()..].chars().next();
        !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
    })
}

/// A job or application reference: "REQ-12345", "Job ID 4411", "Ref. 88213".
fn has_reference(text: &str) -> bool {
    let lower = text.to_lowercase();
    [
        "req",
        "job id",
        "job-id",
        "ref.",
        "ref:",
        "reference",
        "requisition",
        "kennziffer",
    ]
    .iter()
    .any(|marker| {
        lower.match_indices(marker).any(|(i, _)| {
            lower[i + marker.len()..]
                .chars()
                .skip_while(|c| {
                    !c.is_ascii_digit() && (c.is_whitespace() || matches!(c, '-' | '#' | ':' | '.'))
                })
                .take_while(char::is_ascii_digit)
                .count()
                >= 3
        })
    })
}

pub fn score(message: &MailMessage, signals: &Signals) -> Scored {
    let mut score = 0;
    let mut reasons = Vec::new();
    let subject = message.subject.to_lowercase();
    let snippet = message.snippet.to_lowercase();
    let sender = message.sender.to_lowercase();
    let address = message.sender_address().unwrap_or_default();
    let domain = message.sender_domain().unwrap_or_default();

    if signals.known_threads.contains(&message.thread_id) {
        score += 100;
        reasons.push("known application thread");
    }
    if !address.is_empty() && signals.known_senders.contains(&address) {
        score += 60;
        reasons.push("known recruiter");
    }
    if !domain.is_empty()
        && !applications::is_shared_domain(&domain)
        && signals.known_domains.contains(&domain)
    {
        score += 60;
        reasons.push("company of an application");
    }
    if !domain.is_empty() && domain_matches(&domain, ATS_DOMAINS) {
        score += 40;
        reasons.push("applicant tracking system");
    }
    let subject_hit = SUBJECT_WORDS.iter().any(|w| has_word(&subject, w));
    if subject_hit {
        score += 30;
        reasons.push("application words in the subject");
    } else if SUBJECT_WORDS.iter().any(|w| has_word(&snippet, w)) {
        score += 10;
        reasons.push("application words in the text");
    }
    if has_reference(&message.subject) || has_reference(&message.snippet) {
        score += 15;
        reasons.push("reference number");
    }
    let named_company = signals
        .known_companies
        .iter()
        .any(|c| c.len() >= 3 && (has_word(&subject, c) || has_word(&sender, c)));
    if named_company {
        score += 30;
        reasons.push("company of an application named");
    }
    if signals
        .known_roles
        .iter()
        .any(|r| r.len() >= 4 && subject.contains(r.as_str()))
    {
        score += 20;
        reasons.push("role of an application named");
    }
    let bulk = BULK_PHRASES
        .iter()
        .any(|p| subject.contains(p) || snippet.contains(p))
        || BULK_SENDERS.iter().any(|s| address.contains(s));
    if bulk {
        score -= 60;
        reasons.push("job alert or newsletter");
    }

    let verdict = if score >= STRONG {
        Verdict::Strong
    } else if score >= CANDIDATE {
        Verdict::Candidate
    } else {
        Verdict::Filtered
    };
    Scored {
        verdict,
        score,
        reasons,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mail(from: &str, subject: &str, snippet: &str) -> MailMessage {
        MailMessage {
            message_id: "m".into(),
            thread_id: "t".into(),
            sender: from.into(),
            subject: subject.into(),
            snippet: snippet.into(),
            ..MailMessage::default()
        }
    }

    fn signals() -> Signals {
        Signals {
            known_threads: ["t-known".to_string()].into(),
            known_domains: ["acme.io".to_string()].into(),
            known_companies: ["stripe".to_string()].into(),
            known_roles: ["ai engineer".to_string()].into(),
            known_senders: ["tom@recruit.example".to_string()].into(),
        }
    }

    #[test]
    fn strong_signals_skip_the_relevance_check() {
        let s = signals();
        let mut in_thread = mail("x@y.com", "Re: hi", "");
        in_thread.thread_id = "t-known".into();
        assert_eq!(score(&in_thread, &s).verdict, Verdict::Strong);
        assert_eq!(
            score(&mail("Jane <jane@acme.io>", "Quick question", ""), &s).verdict,
            Verdict::Strong
        );
        assert_eq!(
            score(
                &mail("no-reply@greenhouse.io", "Thanks for applying to Beta", ""),
                &s
            )
            .verdict,
            Verdict::Strong
        );
        assert_eq!(
            score(&mail("Tom <tom@recruit.example>", "Coffee?", ""), &s).verdict,
            Verdict::Strong
        );
    }

    #[test]
    fn job_words_make_candidates_and_everything_else_is_filtered() {
        let s = Signals::default();
        assert_eq!(
            score(
                &mail("hr@newco.com", "Your application for Data Engineer", ""),
                &s
            )
            .verdict,
            Verdict::Candidate
        );
        assert_eq!(
            score(
                &mail("people@firm.de", "Einladung zum Vorstellungsgespräch", ""),
                &s
            )
            .verdict,
            Verdict::Candidate
        );
        assert_eq!(
            score(&mail("mom@family.net", "Dinner on Sunday", "see you"), &s).verdict,
            Verdict::Filtered
        );
        assert_eq!(
            score(
                &mail("shop@store.com", "Your order has shipped", "composition"),
                &s
            )
            .verdict,
            Verdict::Filtered,
            "word boundaries: composition is not position"
        );
    }

    #[test]
    fn job_alerts_and_newsletters_are_filtered() {
        let s = Signals::default();
        let alert = score(
            &mail(
                "jobalerts-noreply@linkedin.com",
                "30 new jobs for AI Engineer",
                "",
            ),
            &s,
        );
        assert_eq!(alert.verdict, Verdict::Filtered, "{alert:?}");
        assert_eq!(
            score(&mail("news@site.com", "Newsletter: hiring trends", ""), &s).verdict,
            Verdict::Filtered
        );
    }

    #[test]
    fn reference_numbers_and_known_names_count() {
        assert!(has_reference("Job ID: 44110"));
        assert!(has_reference("REQ-12345"));
        assert!(!has_reference("request received"));
        let s = signals();
        assert!(score(&mail("x@stripe.com", "Stripe update", ""), &s).score >= CANDIDATE);
    }
}
