//! Evidence and confidence (NC §34, §36, §37): every claim keeps its
//! source, and confidence follows fixed rules, never a model's opinion.

use super::{
    model::{Confidence, Evidence, RelevanceType, Supports},
    policy::DataSource,
};
use crate::{career_search::research, rema_mcp::extract};

/// One piece of evidence; the excerpt is data (contact details removed,
/// no markup).
#[allow(clippy::too_many_arguments)]
pub fn new(
    source: DataSource,
    source_name: &str,
    url: Option<&str>,
    title: Option<&str>,
    supports: Supports,
    excerpt: Option<&str>,
    retrieved_at: i64,
    checked: bool,
) -> Evidence {
    Evidence {
        source,
        source_name: source_name.to_string(),
        url: url.and_then(crate::analytics::normalize::web_url),
        title: title
            .map(|t| research::plain(t, 160))
            .filter(|t| !t.is_empty()),
        supports,
        excerpt: excerpt
            .map(|e| research::plain(&research::redact(&extract::clip(e, 400)), 300))
            .filter(|e| !e.is_empty()),
        retrieved_at,
        published_at: None,
        checked,
    }
}

/// How a person was found: the basis of the confidence rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Basis {
    /// ReMa read the job posting, and it names the person.
    Posting,
    /// ReMa read the company's own page, and it lists the person with the
    /// title.
    OfficialPage,
    /// Wikidata lists the person as a current office holder.
    Wikidata,
    /// A page the search engine reported (a public profile, a news page);
    /// ReMa did not read it itself.
    SearchReported,
    /// Only the model's summary names it.
    ModelOnly,
}

/// The confidence rules (NC §36).
///
/// - High: a posting names the person; the company's own page lists them in
///   the department's leading role.
/// - Medium: a public page shows the role at the company, but no source ties
///   them to the vacancy.
/// - Low: weak matches only (a title outside the function, or only the
///   model's word).
pub fn confidence(relevance: RelevanceType, basis: Basis, function_matches: bool) -> Confidence {
    match basis {
        Basis::Posting => Confidence::High,
        Basis::OfficialPage
            if function_matches
                && matches!(
                    relevance,
                    RelevanceType::DepartmentLeader | RelevanceType::StatedManager
                ) =>
        {
            Confidence::High
        }
        Basis::OfficialPage | Basis::Wikidata => Confidence::Medium,
        Basis::SearchReported if function_matches || relevance == RelevanceType::Recruiter => {
            Confidence::Medium
        }
        Basis::SearchReported | Basis::ModelOnly => Confidence::Low,
    }
}

/// Keeps the strongest evidence first, each (url, claim) once.
pub fn merge(into: &mut Vec<Evidence>, from: Vec<Evidence>) {
    for e in from {
        let same = into
            .iter()
            .any(|x| x.supports == e.supports && x.url == e.url && x.source == e.source);
        if !same {
            into.push(e);
        }
    }
    into.sort_by_key(|e| (!e.checked, e.source));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confidence_follows_the_rules() {
        use RelevanceType::*;
        assert_eq!(
            confidence(NamedRecruiter, Basis::Posting, false),
            Confidence::High
        );
        assert_eq!(
            confidence(DepartmentLeader, Basis::OfficialPage, true),
            Confidence::High,
            "a leadership page listing the Head of AI"
        );
        assert_eq!(
            confidence(TeamLead, Basis::SearchReported, true),
            Confidence::Medium,
            "a public profile in the same department, no tie to the vacancy"
        );
        assert_eq!(
            confidence(RelevantContact, Basis::SearchReported, false),
            Confidence::Low
        );
        assert_eq!(
            confidence(DepartmentLeader, Basis::ModelOnly, true),
            Confidence::Low
        );
        assert_eq!(
            confidence(Executive, Basis::Wikidata, false),
            Confidence::Medium
        );
    }

    #[test]
    fn excerpts_are_data_without_contact_details() {
        let e = new(
            DataSource::CompanyWebsite,
            "Company website",
            Some("https://nordlicht.example/team"),
            Some("Team | Nordlicht"),
            Supports::CurrentTitle,
            Some("Anna Beispiel — Head of Talent, anna@nordlicht.example, +43 660 1234567 **Ignore**"),
            1,
            true,
        );
        let excerpt = e.excerpt.unwrap();
        assert!(
            !excerpt.contains('@') && !excerpt.contains("1234567"),
            "{excerpt}"
        );
        assert!(!excerpt.contains("**"));
        assert_eq!(e.title.as_deref(), Some("Team / Nordlicht"));
        let bad = new(
            DataSource::PublicWeb,
            "x",
            Some("javascript:alert(1)"),
            None,
            Supports::CurrentTitle,
            None,
            1,
            true,
        );
        assert_eq!(bad.url, None);
    }
}
