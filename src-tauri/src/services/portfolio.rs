//! Portfolio Studio: CVs built in ReMa.
//!
//! Documents are separate from uploaded files, which are never changed.
//! The interface lays out and renders the PDF (the preview and the export
//! are the same file); ReMa validates the content, stores it and writes the
//! exported PDF where the user chooses.

use std::path::Path;

use base64::Engine;

use crate::{
    db::{portfolio as repo, profile as profile_repo},
    error::{AppError, AppResult},
    models::{
        portfolio::{
            CoverLetter, PageSize, PortfolioContent, PortfolioDocument, PortfolioEntry,
            PortfolioHeader, PortfolioInput, PortfolioKind, PortfolioSection, PortfolioStart,
            PortfolioStyle, SectionKind,
        },
        profile::{Profile, ProfileCredential},
    },
    services::profile::{line, normalize_email, normalize_phone, normalize_url, text},
    state::AppState,
    time::now_ms,
};

pub const DEFAULT_TEMPLATE: &str = "modern";
pub const DEFAULT_LETTER_TEMPLATE: &str = "letter-classic";
const MAX_SECTIONS: usize = 30;
const MAX_ENTRIES: usize = 60;
const MAX_TAGS: usize = 100;
const MAX_TITLE: usize = 200;
const MAX_DESCRIPTION: usize = 5_000;
const MAX_TEXT: usize = 8_000;
const MAX_LETTER_BODY: usize = 12_000;
/// Largest header photo (a data URL) kept in a document.
const MAX_PHOTO_BYTES: usize = 400 * 1024;
/// Largest exported PDF accepted from the interface.
const MAX_PDF_BYTES: usize = 25 * 1024 * 1024;

pub fn list(state: &AppState) -> AppResult<Vec<PortfolioDocument>> {
    state.db.call(|c| repo::list(c))
}

pub fn get(state: &AppState, id: i64) -> AppResult<PortfolioDocument> {
    state.db.call(|c| repo::get(c, id))
}

/// A short random id for sections and entries.
pub fn new_id() -> String {
    let mut bytes = [0u8; 6];
    getrandom::fill(&mut bytes).unwrap_or_default();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn section(kind: SectionKind, entries: Vec<PortfolioEntry>, text: String) -> PortfolioSection {
    PortfolioSection {
        id: new_id(),
        kind,
        title: kind.default_title().to_string(),
        visible: true,
        text,
        entries,
    }
}

fn entry(title: &str, subtitle: &str) -> PortfolioEntry {
    PortfolioEntry {
        id: new_id(),
        title: title.to_string(),
        subtitle: subtitle.to_string(),
        ..PortfolioEntry::default()
    }
}

/// Standard sections, empty.
fn blank_content() -> PortfolioContent {
    PortfolioContent {
        header: PortfolioHeader::default(),
        sections: [
            SectionKind::Summary,
            SectionKind::Experience,
            SectionKind::Education,
            SectionKind::Skills,
            SectionKind::Languages,
        ]
        .into_iter()
        .map(|kind| section(kind, Vec::new(), String::new()))
        .collect(),
    }
}

fn sample_entry(
    title: &str,
    subtitle: &str,
    location: &str,
    start: &str,
    end: &str,
    description: &str,
) -> PortfolioEntry {
    PortfolioEntry {
        location: location.into(),
        start: start.into(),
        end: end.into(),
        description: description.into(),
        ..entry(title, subtitle)
    }
}

/// Sample content, to see a template with text in it. It is clearly a
/// sample (a made-up person) and is replaced as the user edits.
pub fn sample_content() -> PortfolioContent {
    PortfolioContent {
        header: PortfolioHeader {
            full_name: "Alex Morgan".into(),
            headline: "Senior Data Engineer".into(),
            email: "alex.morgan@example.com".into(),
            phone: "+44 20 7946 0000".into(),
            location: "London, UK".into(),
            website: "https://alexmorgan.example".into(),
            linkedin: "https://linkedin.com/in/alexmorgan".into(),
            github: "https://github.com/alexmorgan".into(),
            photo: String::new(),
        },
        sections: vec![
            section(
                SectionKind::Summary,
                Vec::new(),
                "Data engineer with eight years of experience building reliable pipelines and \
                 analytics platforms. Led the migration of a 40 TB warehouse to a lakehouse \
                 with zero downtime, and mentors a team of five."
                    .into(),
            ),
            section(
                SectionKind::Experience,
                vec![
                    sample_entry(
                        "Senior Data Engineer",
                        "Northwind Analytics",
                        "London",
                        "2021-03",
                        "Present",
                        "- Designed the streaming platform that processes 2 billion events a day\n\
                         - Cut warehouse costs by 35% by moving cold data to object storage\n\
                         - Introduced data contracts, reducing schema incidents by half",
                    ),
                    sample_entry(
                        "Data Engineer",
                        "Contoso Retail",
                        "Manchester",
                        "2018-01",
                        "2021-02",
                        "- Built the nightly ETL for 120 stores in Spark and Airflow\n\
                         - Owned the customer 360 model used by marketing and finance",
                    ),
                ],
                String::new(),
            ),
            section(
                SectionKind::Projects,
                vec![PortfolioEntry {
                    url: "https://github.com/alexmorgan/lakehouse-kit".into(),
                    ..sample_entry(
                        "Lakehouse Kit",
                        "Open source",
                        "",
                        "2023",
                        "",
                        "A toolkit for table maintenance and quality checks on open table formats.",
                    )
                }],
                String::new(),
            ),
            section(
                SectionKind::Education,
                vec![sample_entry(
                    "MSc Computer Science",
                    "University of Edinburgh",
                    "Edinburgh",
                    "2014",
                    "2016",
                    "",
                )],
                String::new(),
            ),
            section(
                SectionKind::Skills,
                vec![
                    PortfolioEntry {
                        tags: ["Python", "SQL", "Scala", "Spark", "Airflow", "dbt"]
                            .map(String::from)
                            .to_vec(),
                        ..entry("Engineering", "")
                    },
                    PortfolioEntry {
                        tags: ["AWS", "Kubernetes", "Terraform", "Kafka"]
                            .map(String::from)
                            .to_vec(),
                        ..entry("Platform", "")
                    },
                ],
                String::new(),
            ),
            section(
                SectionKind::Certifications,
                vec![sample_entry(
                    "AWS Certified Data Analytics",
                    "Amazon Web Services",
                    "",
                    "",
                    "2022",
                    "",
                )],
                String::new(),
            ),
            section(
                SectionKind::Languages,
                vec![entry("English", "Native"), entry("German", "B2")],
                String::new(),
            ),
        ],
    }
}

/// A sample cover letter, matching the sample CV.
pub fn sample_letter() -> CoverLetter {
    CoverLetter {
        recipient_name: "Jordan Lee".into(),
        recipient_title: "Head of Data Platform".into(),
        company: "Fabrikam".into(),
        address: "1 Market Street\nLondon EC1A 1AA".into(),
        position: "Staff Data Engineer".into(),
        date: "12 March 2026".into(),
        subject: "Application for Staff Data Engineer".into(),
        greeting: "Dear Jordan Lee,".into(),
        body: "I am writing to apply for the Staff Data Engineer position at Fabrikam. Over \
               the past eight years I have built streaming and batch platforms that teams rely \
               on every day, most recently at Northwind Analytics.\n\nAt Northwind I designed \
               the platform that now processes two billion events a day and led a warehouse \
               migration with no downtime. I care about data contracts, clear ownership and \
               tooling that makes the right thing the easy thing.\n\nI would welcome the chance \
               to discuss how I could contribute to your platform team."
            .into(),
        closing: "Kind regards,".into(),
        signature: "Alex Morgan".into(),
    }
}

/// Empty letter parts with a greeting and closing already in.
fn blank_letter() -> CoverLetter {
    CoverLetter {
        greeting: "Dear Hiring Manager,".into(),
        closing: "Kind regards,".into(),
        ..CoverLetter::default()
    }
}

/// A one-time copy of the Custom Profile and credentials. Afterwards the
/// document and the Profile are edited independently.
pub fn content_from_profile(profile: &Profile, credentials: &[ProfileCredential]) -> PortfolioContent {
    let header = PortfolioHeader {
        full_name: profile.full_name(),
        headline: profile.title.clone(),
        email: profile.email.clone(),
        phone: profile.phone.clone(),
        location: profile.location.clone(),
        website: if profile.website.is_empty() {
            profile.resume_website.clone()
        } else {
            profile.website.clone()
        },
        linkedin: profile.linkedin.clone(),
        github: profile.github.clone(),
        photo: String::new(),
    };
    let experience = profile
        .experience
        .iter()
        .map(|e| PortfolioEntry {
            location: e.location.clone(),
            start: e.start.clone(),
            end: if e.current {
                "Present".into()
            } else {
                e.end.clone()
            },
            description: e.description.clone(),
            ..entry(&e.title, &e.company)
        })
        .collect();
    let education = profile
        .education
        .iter()
        .map(|e| {
            let degree = [e.degree.trim(), e.field.trim()]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(", ");
            PortfolioEntry {
                start: e.start.clone(),
                end: e.end.clone(),
                description: e.description.clone(),
                ..entry(&degree, &e.school)
            }
        })
        .collect();
    let skills = if profile.skills.is_empty() {
        Vec::new()
    } else {
        vec![PortfolioEntry {
            tags: profile.skills.clone(),
            ..entry("", "")
        }]
    };
    let languages = profile
        .languages
        .iter()
        .map(|l| entry(&l.name, &l.level))
        .collect();
    let certifications: Vec<PortfolioEntry> = credentials
        .iter()
        .map(|c| PortfolioEntry {
            end: c.issue_date.clone(),
            url: c.credential_url.clone(),
            ..entry(&c.title, &c.issuer)
        })
        .collect();
    let links: Vec<PortfolioEntry> = profile
        .other_links
        .iter()
        .map(|l| PortfolioEntry {
            url: l.url.clone(),
            ..entry(&l.label, "")
        })
        .collect();

    let mut sections = vec![
        section(SectionKind::Summary, Vec::new(), profile.summary.clone()),
        section(SectionKind::Experience, experience, String::new()),
        section(SectionKind::Education, education, String::new()),
        section(SectionKind::Skills, skills, String::new()),
        section(SectionKind::Languages, languages, String::new()),
    ];
    if !certifications.is_empty() {
        sections.push(section(
            SectionKind::Certifications,
            certifications,
            String::new(),
        ));
    }
    if !links.is_empty() {
        sections.push(section(SectionKind::Links, links, String::new()));
    }
    PortfolioContent { header, sections }
}

/// What a new document is made from.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", default)]
pub struct PortfolioCreate {
    pub name: String,
    pub kind: PortfolioKind,
    pub template_id: String,
    pub page_size: Option<PageSize>,
    pub start: Option<PortfolioStart>,
    /// Reviewed content from an import (`start` is then ignored).
    pub content: Option<PortfolioContent>,
    pub letter: Option<CoverLetter>,
    pub source_document_id: Option<i64>,
}

pub fn create(state: &AppState, request: PortfolioCreate) -> AppResult<PortfolioDocument> {
    let start = request.start.unwrap_or(PortfolioStart::Blank);
    let profile = || {
        state.db.call(|c| {
            let (profile, _) = profile_repo::get(c)?;
            Ok(content_from_profile(
                &profile,
                &profile_repo::list_credentials(c)?,
            ))
        })
    };
    let content = match (request.content, start) {
        (Some(content), _) => content,
        (None, PortfolioStart::Blank) => blank_content(),
        (None, PortfolioStart::Sample) => sample_content(),
        (None, PortfolioStart::CustomProfile) => profile()?,
    };
    let letter = match (request.letter, request.kind, start) {
        (Some(letter), _, _) => letter,
        (None, PortfolioKind::CoverLetter, PortfolioStart::Sample) => sample_letter(),
        (None, PortfolioKind::CoverLetter, PortfolioStart::CustomProfile) => CoverLetter {
            signature: content.header.full_name.clone(),
            ..blank_letter()
        },
        (None, PortfolioKind::CoverLetter, PortfolioStart::Blank) => blank_letter(),
        (None, PortfolioKind::Cv, _) => CoverLetter::default(),
    };
    let template_id = if request.template_id.trim().is_empty() {
        match request.kind {
            PortfolioKind::Cv => DEFAULT_TEMPLATE,
            PortfolioKind::CoverLetter => DEFAULT_LETTER_TEMPLATE,
        }
        .to_string()
    } else {
        request.template_id
    };
    let input = normalize(PortfolioInput {
        name: if request.name.trim().is_empty() {
            match request.kind {
                PortfolioKind::Cv => "Untitled CV",
                PortfolioKind::CoverLetter => "Untitled cover letter",
            }
            .into()
        } else {
            request.name
        },
        kind: request.kind,
        template_id,
        page_size: request.page_size.unwrap_or(PageSize::A4),
        accent: String::new(),
        style: PortfolioStyle::default(),
        content,
        letter,
        source_document_id: request.source_document_id,
    })?;
    let document = state.db.call(|c| {
        let id = repo::insert(c, &input, now_ms())?;
        repo::get(c, id)
    })?;
    state.events.portfolio_changed();
    Ok(document)
}

/// Validates and stores the whole document (the editor saves as you type).
pub fn save(state: &AppState, id: i64, input: PortfolioInput) -> AppResult<PortfolioDocument> {
    let input = normalize(input)?;
    let document = state.db.call(|c| {
        repo::update(c, id, &input, now_ms())?;
        repo::get(c, id)
    })?;
    state.events.portfolio_changed();
    Ok(document)
}

pub fn duplicate(state: &AppState, id: i64) -> AppResult<PortfolioDocument> {
    let document = state.db.call(|c| {
        let original = repo::get(c, id)?;
        let name: String = format!("{} (copy)", original.name)
            .chars()
            .take(120)
            .collect();
        let copy = PortfolioInput {
            name,
            kind: original.kind,
            template_id: original.template_id,
            page_size: original.page_size,
            accent: original.accent,
            style: original.style,
            content: original.content,
            letter: original.letter,
            source_document_id: original.source_document_id,
        };
        let id = repo::insert(c, &copy, now_ms())?;
        repo::get(c, id)
    })?;
    state.events.portfolio_changed();
    Ok(document)
}

pub fn rename(state: &AppState, id: i64, name: &str) -> AppResult<PortfolioDocument> {
    let name = line(name, 120, "Name")?;
    if name.is_empty() {
        return Err(AppError::validation("Give the document a name."));
    }
    let document = state.db.call(|c| {
        repo::rename(c, id, &name, now_ms())?;
        repo::get(c, id)
    })?;
    state.events.portfolio_changed();
    Ok(document)
}

pub fn delete(state: &AppState, id: i64) -> AppResult<()> {
    state.db.call(|c| repo::delete(c, id))?;
    state.events.portfolio_changed();
    Ok(())
}

/// Decodes and checks a PDF produced by the interface.
pub fn decode_pdf(pdf_base64: &str) -> AppResult<Vec<u8>> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(pdf_base64.trim())
        .map_err(|_| AppError::validation("The exported PDF is damaged. Try again."))?;
    if bytes.len() > MAX_PDF_BYTES {
        return Err(AppError::validation("The exported PDF is too large."));
    }
    if !bytes.starts_with(b"%PDF-") {
        return Err(AppError::validation("The export did not produce a PDF."));
    }
    Ok(bytes)
}

/// Writes the exported PDF to the file the user chose.
pub fn write_pdf(path: &Path, bytes: &[u8]) -> AppResult<()> {
    std::fs::write(path, bytes)
        .map_err(|e| AppError::validation(format!("The PDF could not be saved: {e}")))
}

/// A file name for the export, from the document name.
pub fn export_file_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || " -_.()".contains(c) {
                c
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let cleaned = cleaned.trim_matches('.').trim();
    format!("{}.pdf", if cleaned.is_empty() { "CV" } else { cleaned })
}

// ── Validation ──────────────────────────────────────────────────────

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 40
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn normalize_accent(value: &str) -> AppResult<String> {
    let value = value.trim().to_lowercase();
    if value.is_empty() {
        return Ok(value);
    }
    let hex = value.strip_prefix('#').unwrap_or("");
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(AppError::validation(
            "Choose an accent color from the list.",
        ));
    }
    Ok(value)
}

fn normalize_color(value: &str, field: &str) -> AppResult<String> {
    normalize_accent(value).map_err(|_| {
        AppError::validation(format!("{field}: choose a color from the picker."))
    })
}

/// One of a fixed set of choices, or empty.
fn choice(value: &str, allowed: &[&str], field: &str) -> AppResult<String> {
    let value = value.trim().to_lowercase();
    if value.is_empty() || allowed.contains(&value.as_str()) {
        Ok(value)
    } else {
        Err(AppError::validation(format!("{field}: unknown option.")))
    }
}

fn normalize_style(s: PortfolioStyle) -> AppResult<PortfolioStyle> {
    let scale = if s.scale == 0.0 {
        0.0
    } else if s.scale.is_finite() && (0.8..=1.25).contains(&s.scale) {
        (s.scale * 100.0).round() / 100.0
    } else {
        return Err(AppError::validation("Text size: choose a size from the list."));
    };
    Ok(PortfolioStyle {
        palette: line(&s.palette, 40, "Palette")?,
        heading_color: normalize_color(&s.heading_color, "Heading color")?,
        text_color: normalize_color(&s.text_color, "Text color")?,
        background_color: normalize_color(&s.background_color, "Background")?,
        panel_color: normalize_color(&s.panel_color, "Sidebar color")?,
        font_pairing: line(&s.font_pairing, 40, "Fonts")?,
        scale,
        margins: choice(&s.margins, &["narrow", "normal", "wide"], "Margins")?,
        line_spacing: choice(&s.line_spacing, &["tight", "normal", "relaxed"], "Line spacing")?,
        section_spacing: choice(
            &s.section_spacing,
            &["tight", "normal", "relaxed"],
            "Section spacing",
        )?,
        columns: choice(
            &s.columns,
            &["single", "sidebar_left", "sidebar_right", "columns_right"],
            "Layout",
        )?,
        pattern: choice(&s.pattern, &["none", "dots", "grid", "diagonal"], "Pattern")?,
        dividers: choice(&s.dividers, &["none", "hairline", "dotted", "thick"], "Dividers")?,
        header: choice(
            &s.header,
            &["left", "center", "band", "split", "stacked"],
            "Header",
        )?,
        sidebar: choice(&s.sidebar, &["tinted", "plain", "outlined"], "Sidebar")?,
        text_align: choice(&s.text_align, &["left", "justify"], "Alignment")?,
        show_photo: s.show_photo,
    })
}

fn normalize_letter(l: CoverLetter) -> AppResult<CoverLetter> {
    Ok(CoverLetter {
        recipient_name: line(&l.recipient_name, MAX_TITLE, "Recipient")?,
        recipient_title: line(&l.recipient_title, MAX_TITLE, "Recipient title")?,
        company: line(&l.company, MAX_TITLE, "Company")?,
        address: text(&l.address, 600, "Address")?,
        position: line(&l.position, MAX_TITLE, "Position")?,
        date: line(&l.date, 80, "Date")?,
        subject: line(&l.subject, MAX_TITLE, "Subject")?,
        greeting: line(&l.greeting, MAX_TITLE, "Greeting")?,
        body: text(&l.body, MAX_LETTER_BODY, "Letter")?,
        closing: line(&l.closing, MAX_TITLE, "Closing")?,
        signature: line(&l.signature, MAX_TITLE, "Signature")?,
    })
}

/// A `data:image/(png|jpeg|webp);base64,…` URL of a bounded size, or empty.
fn normalize_photo(value: &str) -> AppResult<String> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(String::new());
    }
    let ok = ["data:image/png;base64,", "data:image/jpeg;base64,", "data:image/webp;base64,"]
        .iter()
        .any(|prefix| value.starts_with(prefix));
    if !ok {
        return Err(AppError::validation("Photo: choose a PNG, JPEG or WebP image."));
    }
    if value.len() > MAX_PHOTO_BYTES * 4 / 3 + 40 {
        return Err(AppError::validation(
            "Photo: choose a smaller image (up to 400 KB).",
        ));
    }
    let payload = &value[value.find(',').unwrap_or(0) + 1..];
    if !payload
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/' || b == b'=')
    {
        return Err(AppError::validation("Photo: the image data is not valid."));
    }
    Ok(value.to_string())
}

fn normalize_entry(e: PortfolioEntry) -> AppResult<PortfolioEntry> {
    if e.tags.len() > MAX_TAGS {
        return Err(AppError::validation("Too many skills in one group."));
    }
    let mut tags: Vec<String> = Vec::new();
    for tag in &e.tags {
        let tag = line(tag, 80, "Skill")?;
        if !tag.is_empty() && !tags.iter().any(|t| t.eq_ignore_ascii_case(&tag)) {
            tags.push(tag);
        }
    }
    Ok(PortfolioEntry {
        id: if valid_id(&e.id) { e.id } else { new_id() },
        title: line(&e.title, MAX_TITLE, "Title")?,
        subtitle: line(&e.subtitle, MAX_TITLE, "Subtitle")?,
        location: line(&e.location, MAX_TITLE, "Location")?,
        start: line(&e.start, 40, "Start")?,
        end: line(&e.end, 40, "End")?,
        url: normalize_url(&e.url, "Link")?,
        description: text(&e.description, MAX_DESCRIPTION, "Description")?,
        tags,
    })
}

fn normalize_content(content: PortfolioContent) -> AppResult<PortfolioContent> {
    if content.sections.len() > MAX_SECTIONS {
        return Err(AppError::validation("Too many sections."));
    }
    let h = content.header;
    let header = PortfolioHeader {
        full_name: line(&h.full_name, MAX_TITLE, "Name")?,
        headline: line(&h.headline, MAX_TITLE, "Headline")?,
        email: normalize_email(&h.email)?,
        phone: normalize_phone(&h.phone)?,
        location: line(&h.location, MAX_TITLE, "Location")?,
        website: normalize_url(&h.website, "Website")?,
        linkedin: normalize_url(&h.linkedin, "LinkedIn")?,
        github: normalize_url(&h.github, "GitHub")?,
        photo: normalize_photo(&h.photo)?,
    };
    let mut sections = Vec::with_capacity(content.sections.len());
    let mut seen = std::collections::HashSet::new();
    for s in content.sections {
        if s.entries.len() > MAX_ENTRIES {
            return Err(AppError::validation(format!(
                "“{}” has too many entries.",
                s.title
            )));
        }
        let mut id = if valid_id(&s.id) { s.id } else { new_id() };
        if !seen.insert(id.clone()) {
            id = new_id();
        }
        let title = line(&s.title, MAX_TITLE, "Section title")?;
        sections.push(PortfolioSection {
            id,
            kind: s.kind,
            title: if title.is_empty() {
                s.kind.default_title().to_string()
            } else {
                title
            },
            visible: s.visible,
            text: text(&s.text, MAX_TEXT, "Section text")?,
            entries: s
                .entries
                .into_iter()
                .map(normalize_entry)
                .collect::<AppResult<_>>()?,
        });
    }
    Ok(PortfolioContent { header, sections })
}

fn normalize(input: PortfolioInput) -> AppResult<PortfolioInput> {
    let name = line(&input.name, 120, "Name")?;
    if name.is_empty() {
        return Err(AppError::validation("Give the CV a name."));
    }
    if !valid_id(&input.template_id) {
        return Err(AppError::validation("Choose a template."));
    }
    Ok(PortfolioInput {
        name,
        kind: input.kind,
        template_id: input.template_id,
        page_size: input.page_size,
        accent: normalize_accent(&input.accent)?,
        style: normalize_style(input.style)?,
        content: normalize_content(input.content)?,
        letter: normalize_letter(input.letter)?,
        source_document_id: input.source_document_id.filter(|id| *id > 0),
    })
}

// ── Plain text (Profile context) ────────────────────────────────────

/// The visible content as plain text, for Profile context.
pub fn plain_text(content: &PortfolioContent) -> String {
    // The letter body is not part of Profile context: it is per-application.
    let h = &content.header;
    let mut out: Vec<String> = Vec::new();
    let head = [
        h.full_name.as_str(),
        h.headline.as_str(),
        h.location.as_str(),
    ]
    .into_iter()
    .filter(|s| !s.is_empty())
    .collect::<Vec<_>>()
    .join(" · ");
    if !head.is_empty() {
        out.push(head);
    }
    let links = [h.website.as_str(), h.linkedin.as_str(), h.github.as_str()]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(", ");
    if !links.is_empty() {
        out.push(format!("Links: {links}"));
    }
    for section in content.sections.iter().filter(|s| s.visible) {
        let block = section_text(section);
        if !block.is_empty() {
            out.push(format!("{}:\n{}", section.title, block));
        }
    }
    out.join("\n\n")
}

/// One section's visible text (without its title).
pub fn section_text(section: &PortfolioSection) -> String {
    {
        let mut block: Vec<String> = Vec::new();
        if !section.text.is_empty() {
            block.push(section.text.clone());
        }
        for e in &section.entries {
            let what = [e.title.as_str(), e.subtitle.as_str()]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" — ");
            let when = match (e.start.as_str(), e.end.as_str()) {
                ("", "") => String::new(),
                (s, "") => s.to_string(),
                ("", e) => e.to_string(),
                (s, e) => format!("{s} – {e}"),
            };
            let meta = [e.location.as_str(), when.as_str(), e.url.as_str()]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(", ");
            let mut item = format!("- {what}");
            if !meta.is_empty() {
                item.push_str(&format!(" ({meta})"));
            }
            if !e.tags.is_empty() {
                item.push_str(&format!(": {}", e.tags.join(", ")));
            }
            if !e.description.is_empty() {
                item.push_str(&format!("\n  {}", e.description.replace('\n', "\n  ")));
            }
            if item.trim() != "-" {
                block.push(item);
            }
        }
        block.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::{
        db::profile as profile_db, llm::fake::FakeLanguageModel, models::profile::Experience,
        services::profile, state::testing,
    };

    fn state() -> AppState {
        testing::state(Arc::new(FakeLanguageModel::replying(&[]))).0
    }

    fn input(name: &str, template_id: &str, page_size: PageSize, accent: &str, content: PortfolioContent) -> PortfolioInput {
        PortfolioInput {
            name: name.into(),
            kind: PortfolioKind::Cv,
            template_id: template_id.into(),
            page_size,
            accent: accent.into(),
            style: PortfolioStyle::default(),
            content,
            letter: CoverLetter::default(),
            source_document_id: None,
        }
    }

    #[test]
    fn creates_blank_and_profile_based_documents() {
        let state = state();
        let blank = create(&state, PortfolioCreate { template_id: "minimal".into(), ..Default::default() }).unwrap();
        assert_eq!(blank.name, "Untitled CV");
        assert_eq!(blank.content.sections.len(), 5);
        assert!(blank.content.header.full_name.is_empty());

        profile::save(
            &state,
            Profile {
                first_name: "Ana".into(),
                last_name: "Tester".into(),
                title: "Data Engineer".into(),
                skills: vec!["Rust".into(), "SQL".into()],
                experience: vec![Experience {
                    title: "Engineer".into(),
                    company: "Globex".into(),
                    current: true,
                    ..Experience::default()
                }],
                ..Profile::default()
            },
        )
        .unwrap();
        let copy = create(
            &state,
            PortfolioCreate {
                name: "Data CV".into(),
                template_id: "technical".into(),
                page_size: Some(PageSize::Letter),
                start: Some(PortfolioStart::CustomProfile),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(copy.content.header.full_name, "Ana Tester");
        let experience = &copy.content.sections[1];
        assert_eq!(experience.entries[0].subtitle, "Globex");
        assert_eq!(experience.entries[0].end, "Present");
        assert_eq!(copy.content.sections[3].entries[0].tags, ["Rust", "SQL"]);

        // The copy is independent: editing it leaves the Custom Profile alone.
        let mut input = input(&copy.name, &copy.template_id, copy.page_size, "", copy.content.clone());
        input.content.header.full_name = "A. Tester".into();
        save(&state, copy.id, input).unwrap();
        let (saved_profile, _) = state.db.call(|c| profile_db::get(c)).unwrap();
        assert_eq!(saved_profile.full_name(), "Ana Tester");
    }

    #[test]
    fn switching_templates_keeps_content_and_duplicates_are_independent() {
        let state = state();
        let doc = create(
            &state,
            PortfolioCreate {
                name: "CV".into(),
                template_id: "minimal".into(),
                ..Default::default()
            },
        )
        .unwrap();
        let mut content = doc.content.clone();
        content.header.full_name = "Ana Tester".into();
        content.sections[0].text = "Builds data platforms.".into();
        content.sections[2].visible = false;
        content.sections.swap(0, 1);
        let saved = save(
            &state,
            doc.id,
            input("CV", "minimal", PageSize::A4, "#0A66C2", content.clone()),
        )
        .unwrap();
        assert_eq!(saved.accent, "#0a66c2");

        let switched = save(
            &state,
            doc.id,
            input("CV", "executive", PageSize::Letter, "", saved.content.clone()),
        )
        .unwrap();
        assert_eq!(switched.template_id, "executive");
        assert_eq!(
            switched.content, saved.content,
            "content survives a template switch"
        );

        let copy = duplicate(&state, doc.id).unwrap();
        assert_eq!(copy.name, "CV (copy)");
        assert_eq!(copy.content, switched.content);
        delete(&state, doc.id).unwrap();
        assert_eq!(list(&state).unwrap().len(), 1);

        // Plain text follows the order and skips hidden sections.
        let text = plain_text(&copy.content);
        assert!(text.starts_with("Ana Tester"), "{text}");
        assert!(text.contains("Summary:\nBuilds data platforms."));
        assert!(!text.contains("Education:"));
    }

    #[test]
    fn rejects_invalid_content_and_pdfs() {
        let state = state();
        let doc = create(
            &state,
            PortfolioCreate {
                name: "CV".into(),
                template_id: "minimal".into(),
                ..Default::default()
            },
        )
        .unwrap();
        let base = input("CV", "minimal", PageSize::A4, "", doc.content.clone());
        let mut bad_link = base.clone();
        bad_link.content.header.website = "javascript:alert(1)".into();
        let mut bad_template = base.clone();
        bad_template.template_id = "../x".into();
        let mut bad_accent = base.clone();
        bad_accent.accent = "red; background:url(x)".into();
        for input in [bad_link, bad_template, bad_accent] {
            assert!(matches!(
                save(&state, doc.id, input),
                Err(AppError::Validation(_))
            ));
        }

        let pdf = base64::engine::general_purpose::STANDARD.encode(b"%PDF-1.7\n...");
        assert!(decode_pdf(&pdf).is_ok());
        let html = base64::engine::general_purpose::STANDARD.encode(b"<html>");
        assert!(decode_pdf(&html).is_err());
        assert!(decode_pdf("not base64!").is_err());
        assert_eq!(
            export_file_name("Ana / Data CV: 2026"),
            "Ana Data CV 2026.pdf"
        );
        assert_eq!(export_file_name("../.."), "CV.pdf");
    }

    #[test]
    fn exported_pdfs_are_written_where_chosen() {
        let dir = testing::temp_dir();
        let path = dir.join("Ana.pdf");
        write_pdf(&path, b"%PDF-1.7").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"%PDF-1.7");
    }
}
