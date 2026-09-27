//! Which ProfileContext field a label belongs to ("Preferred location",
//! "Skills", "Salary expectation", …). Used for Custom Profile fields, CV
//! headings and `Label: value` lines in CVs. Deterministic word matching.

use super::Field;

/// What a label means for the context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Label {
    Field(Field),
    /// Contact or sensitive personal details: never sent.
    Skip,
    Unknown,
}

/// Phrases per class, checked in this order (the first match wins). A
/// phrase matches whole words; a trailing `*` matches a word prefix.
const RULES: &[(Label, &[&str])] = &[
    (
        Label::Skip,
        &[
            "email",
            "e mail",
            "phone",
            "mobile",
            "tel",
            "telephone",
            "telefon",
            "handy",
            "address",
            "adresse",
            "anschrift",
            "birth*",
            "born",
            "geburt*",
            "age",
            "alter",
            "nationality",
            "citizenship",
            "staatsangehörigkeit",
            "marital status",
            "familienstand",
            "gender",
            "geschlecht",
            "religion",
            "photo",
            "contact",
            "contact details",
            "kontakt",
            "personal details",
            "personal information",
            "persönliche daten",
            "references",
            "referenzen",
        ],
    ),
    (
        Label::Field(Field::TargetRoles),
        &[
            "target*",
            "desired role*",
            "desired position*",
            "desired job*",
            "seeking",
            "looking for",
            "career goal*",
            "career objective",
            "objective",
            "aspiration*",
            "next role",
            "dream job",
            "zielposition",
            "wunschposition",
            "berufsziel",
        ],
    ),
    (
        Label::Field(Field::SalaryPreferences),
        &[
            "salary",
            "salaries",
            "compensation",
            "pay",
            "wage",
            "day rate",
            "hourly rate",
            "rate",
            "gehalt",
            "gehaltsvorstellung",
            "income",
            "ctc",
        ],
    ),
    (
        Label::Field(Field::WorkPreferences),
        &[
            "remote",
            "hybrid",
            "on site",
            "onsite",
            "work mode",
            "work preference*",
            "working preference*",
            "work style",
            "employment type",
            "job type",
            "contract*",
            "availability",
            "available",
            "notice period",
            "start date",
            "earliest start",
            "kündigungsfrist",
            "visa",
            "work permit",
            "travel",
            "working hours",
            "part time",
            "full time",
            "arbeitszeit",
            "company size",
            "team size",
        ],
    ),
    (
        Label::Field(Field::Technologies),
        &[
            "technolog*",
            "tech stack",
            "stack",
            "tools",
            "tooling",
            "framework*",
            "programming*",
            "platforms",
            "software",
            "technical environment",
        ],
    ),
    (
        Label::Field(Field::Languages),
        &["language*", "sprachen", "sprachkenntnisse", "fremdsprachen"],
    ),
    (
        Label::Field(Field::Certifications),
        &[
            "certif*",
            "licen*",
            "course*",
            "training*",
            "zertifikat*",
            "weiterbildung*",
        ],
    ),
    (
        Label::Field(Field::Skills),
        &[
            "skill*",
            "competenc*",
            "expertise",
            "strengths",
            "kenntnisse",
            "fähigkeiten",
            "kompetenzen",
            "qualifications",
        ],
    ),
    (
        Label::Field(Field::Education),
        &[
            "education",
            "academic*",
            "degree*",
            "university",
            "studies",
            "ausbildung",
            "studium",
            "bildung*",
        ],
    ),
    (
        Label::Field(Field::Experience),
        &[
            "experience",
            "work experience",
            "professional experience",
            "employment*",
            "work history",
            "career history",
            "career",
            "berufserfahrung",
            "berufliche*",
            "positions held",
        ],
    ),
    (
        Label::Field(Field::Projects),
        &["project*", "side project*", "open source", "projekte"],
    ),
    (
        Label::Field(Field::Achievements),
        &[
            "achievement*",
            "award*",
            "honors",
            "honours",
            "publication*",
            "accomplishment*",
            "patent*",
            "erfolge",
            "auszeichnung*",
        ],
    ),
    (
        Label::Field(Field::Industries),
        &["industr*", "sector*", "domain*", "branche*"],
    ),
    (
        Label::Field(Field::Locations),
        &[
            "location*",
            "relocat*",
            "city",
            "cities",
            "based in",
            "wohnort",
            "standort*",
            "country",
            "countries",
            "region*",
            "where",
        ],
    ),
    (
        Label::Field(Field::PortfolioInformation),
        &[
            "portfolio",
            "website*",
            "github",
            "gitlab",
            "linkedin",
            "blog",
            "homepage",
            "personal site",
            "behance",
            "dribbble",
            "kaggle",
            "scholar",
        ],
    ),
    (
        Label::Field(Field::Summary),
        &[
            "summary",
            "about",
            "about me",
            "profile",
            "professional profile",
            "overview",
            "bio",
            "introduction",
            "über mich",
            "kurzprofil",
        ],
    ),
    (
        Label::Field(Field::AdditionalRelevantContext),
        &[
            "interest*",
            "hobb*",
            "volunteer*",
            "activities",
            "additional*",
            "other",
            "miscellaneous",
            "ehrenamt*",
        ],
    ),
    (
        Label::Field(Field::Identity),
        &[
            "name",
            "full name",
            "title",
            "headline",
            "current role",
            "current position",
        ],
    ),
];

/// Lowercase words of a label.
fn words(label: &str) -> Vec<String> {
    label
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

fn matches(label: &[String], phrase: &str) -> bool {
    let (phrase, prefix) = match phrase.strip_suffix('*') {
        Some(stem) => (stem, true),
        None => (phrase, false),
    };
    let parts: Vec<&str> = phrase.split(' ').collect();
    if parts.len() > label.len() {
        return false;
    }
    label.windows(parts.len()).any(|window| {
        window
            .iter()
            .zip(&parts)
            .enumerate()
            .all(|(i, (word, part))| {
                if prefix && i == parts.len() - 1 {
                    word.starts_with(part)
                } else {
                    word == part
                }
            })
    })
}

/// The class of a label.
pub fn classify(label: &str) -> Label {
    let label = words(label);
    if label.is_empty() {
        return Label::Unknown;
    }
    RULES
        .iter()
        .find(|(_, phrases)| phrases.iter().any(|p| matches(&label, p)))
        .map_or(Label::Unknown, |(class, _)| *class)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_common_labels() {
        use Field::*;
        for (label, expected) in [
            ("Preferred location", Label::Field(Locations)),
            ("Target roles", Label::Field(TargetRoles)),
            ("Salary expectation", Label::Field(SalaryPreferences)),
            ("Remote preference", Label::Field(WorkPreferences)),
            ("Notice period", Label::Field(WorkPreferences)),
            ("Programming languages", Label::Field(Technologies)),
            ("Languages", Label::Field(Languages)),
            ("TECHNICAL SKILLS", Label::Field(Skills)),
            ("Work Experience", Label::Field(Experience)),
            ("Licenses & Certifications", Label::Field(Certifications)),
            ("Industries", Label::Field(Industries)),
            ("Date of birth", Label::Skip),
            ("E-Mail", Label::Skip),
            (
                "Corporate volunteering",
                Label::Field(AdditionalRelevantContext),
            ),
            ("Favourite colour", Label::Unknown),
        ] {
            assert_eq!(classify(label), expected, "{label}");
        }
    }
}
