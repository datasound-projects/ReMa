//! ReMa Auto Fill: fills common application fields from the profile.
//!
//! Only runs when the user presses "ReMa Auto Fill":
//! 1. A small script lists the page's visible form fields (labels, names,
//!    `autocomplete`, types; never their values).
//! 2. Rust decides deterministically which profile value fits each field.
//! 3. A second script sets exactly those values in empty fields and fires
//!    the usual input events. Nothing else from the profile reaches the page.
//!
//! The scripts never click, press keys or submit, and never touch password,
//! checkbox or radio fields. The page's answer is untrusted input: it is
//! size-limited and parsed into strict types. Files are attached only when
//! the user picks a document for a specific upload field.

use std::{sync::Mutex, time::Duration};

use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Deserialize;
use tauri::AppHandle;

use crate::{
    browser::{current_url, LABEL},
    db::profile as profile_repo,
    error::{AppError, AppResult},
    models::{
        browser::{AutofillResult, FileField, FileFieldKind, FilledField},
        profile::Profile,
    },
    services::profile as profile_service,
    state::AppState,
};

const SCRIPT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_ANSWER_BYTES: usize = 2 * 1024 * 1024;
const MAX_FIELDS: usize = 300;
const MAX_TEXT: usize = 300;
/// Largest file attached to a form.
const MAX_ATTACH_BYTES: usize = 10 * 1024 * 1024;

/// The last scan of the current page.
pub struct Session {
    nonce: String,
    url: String,
    files: Vec<FileField>,
}

// ── Page scripts ────────────────────────────────────────────────────

/// Lists fillable fields and keeps references to them under a random key.
fn scan_script(nonce: &str) -> String {
    format!(
        r#"(() => {{ try {{
  const KEY = "__remaAutofill_{nonce}";
  const clean = (s) => String(s || "").replace(/\s+/g, " ").trim().slice(0, 300);
  const visible = (el) => {{
    const r = el.getBoundingClientRect(); const s = getComputedStyle(el);
    return r.width > 0 && r.height > 0 && s.visibility !== "hidden" && s.display !== "none";
  }};
  const labelOf = (el) => {{
    let t = el.labels && el.labels.length ? Array.from(el.labels).map((l) => l.innerText).join(" ") : "";
    const by = el.getAttribute("aria-labelledby");
    if (!t && by) t = by.split(/\s+/).map((id) => (document.getElementById(id) || {{}}).innerText || "").join(" ");
    if (!t && el.closest("label")) t = el.closest("label").innerText;
    return clean(t);
  }};
  const nearby = (el) => {{
    const legend = el.closest("fieldset") && el.closest("fieldset").querySelector("legend");
    if (legend) return clean(legend.innerText);
    let node = el;
    for (let depth = 0; depth < 3 && node; depth++) {{
      for (let p = node.previousElementSibling; p; p = p.previousElementSibling) {{
        const t = clean(p.innerText);
        if (t && t.length < 160) return t;
      }}
      node = node.parentElement;
    }}
    return "";
  }};
  const skip = ["hidden", "submit", "button", "reset", "image", "password", "checkbox", "radio", "search", "range", "color"];
  const fields = Array.from(document.querySelectorAll("input, textarea, select")).filter((el) => {{
    const type = String(el.type || "").toLowerCase();
    if (skip.includes(type) || el.disabled || el.readOnly) return false;
    return type === "file" || visible(el);
  }}).slice(0, {MAX_FIELDS});
  Object.defineProperty(window, KEY, {{ value: fields, configurable: true }});
  const frames = Array.from(document.querySelectorAll("iframe")).map((f) => {{
    try {{ return f.contentDocument ? "" : f.src; }} catch (e) {{ return f.src; }}
  }}).filter((s) => /^https?:/.test(s)).slice(0, 10);
  return {{
    url: location.href,
    frames,
    fields: fields.map((el, i) => ({{
      i,
      tag: el.tagName.toLowerCase(),
      type: String(el.type || "").toLowerCase(),
      name: clean(el.name), id: clean(el.id),
      autocomplete: clean(el.getAttribute("autocomplete")),
      label: labelOf(el), placeholder: clean(el.placeholder),
      aria: clean(el.getAttribute("aria-label")), nearby: nearby(el),
      empty: el.tagName === "SELECT" ? el.selectedIndex <= 0 : el.type === "file" ? !(el.files && el.files.length) : !String(el.value || "").trim(),
      options: el.tagName === "SELECT" ? Array.from(el.options).slice(0, 300).map((o) => clean(o.text)) : [],
    }})),
  }};
}} catch (e) {{ return {{ error: String(e) }}; }} }})()"#
    )
}

/// One value to set: field index, text value, or option index for selects.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Fill {
    pub index: u32,
    pub value: String,
    pub option: Option<usize>,
}

/// Sets values like a user would (native setter + input/change events).
fn fill_script(nonce: &str, fills: &[Fill]) -> String {
    let plan = serde_json::to_string(fills).unwrap_or_else(|_| "[]".into());
    format!(
        r#"(() => {{ try {{
  const fields = window["__remaAutofill_{nonce}"];
  if (!fields) return {{ error: "stale" }};
  const plan = {plan};
  const done = [];
  for (const step of plan) {{
    const el = fields[step.index];
    if (!el || !el.isConnected) {{ done.push(false); continue; }}
    if (el.tagName === "SELECT") {{
      if (step.option === null || step.option >= el.options.length) {{ done.push(false); continue; }}
      el.selectedIndex = step.option;
    }} else {{
      const proto = el.tagName === "TEXTAREA" ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
      Object.getOwnPropertyDescriptor(proto, "value").set.call(el, step.value);
    }}
    el.dispatchEvent(new Event("input", {{ bubbles: true }}));
    el.dispatchEvent(new Event("change", {{ bubbles: true }}));
    const before = el.style.boxShadow;
    el.style.boxShadow = "0 0 0 2px rgba(10, 102, 194, 0.55)";
    setTimeout(() => {{ el.style.boxShadow = before; }}, 2500);
    done.push(true);
  }}
  return {{ done }};
}} catch (e) {{ return {{ error: String(e) }}; }} }})()"#
    )
}

/// Puts one file into an upload field, as if the user had chosen it.
fn attach_script(nonce: &str, index: u32, name: &str, mime: &str, data: &str) -> String {
    let name = serde_json::to_string(name).unwrap_or_else(|_| "\"document\"".into());
    let mime = serde_json::to_string(mime).unwrap_or_else(|_| "\"\"".into());
    format!(
        r#"(() => {{ try {{
  const fields = window["__remaAutofill_{nonce}"];
  const el = fields && fields[{index}];
  if (!el || !el.isConnected || el.type !== "file") return {{ ok: false, error: "field" }};
  const raw = atob("{data}");
  const bytes = new Uint8Array(raw.length);
  for (let i = 0; i < raw.length; i++) bytes[i] = raw.charCodeAt(i);
  const transfer = new DataTransfer();
  transfer.items.add(new File([bytes], {name}, {{ type: {mime} }}));
  el.files = transfer.files;
  el.dispatchEvent(new Event("input", {{ bubbles: true }}));
  el.dispatchEvent(new Event("change", {{ bubbles: true }}));
  return {{ ok: el.files.length === 1 && el.files[0].name === {name} }};
}} catch (e) {{ return {{ ok: false, error: String(e) }}; }} }})()"#
    )
}

// ── Page answers (untrusted) ────────────────────────────────────────

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct FieldInfo {
    pub i: u32,
    pub tag: String,
    #[serde(rename = "type")]
    pub input_type: String,
    pub name: String,
    pub id: String,
    pub autocomplete: String,
    pub label: String,
    pub placeholder: String,
    pub aria: String,
    pub nearby: String,
    pub empty: bool,
    pub options: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ScanAnswer {
    url: String,
    fields: Vec<FieldInfo>,
    frames: Vec<String>,
    error: Option<String>,
}

fn cut(value: &mut String) {
    if value.chars().count() > MAX_TEXT {
        *value = value.chars().take(MAX_TEXT).collect();
    }
}

fn parse_scan(raw: &str) -> AppResult<ScanAnswer> {
    if raw.len() > MAX_ANSWER_BYTES {
        return Err(AppError::validation("This page has too many form fields."));
    }
    let mut answer: ScanAnswer = serde_json::from_str(raw)
        .map_err(|_| AppError::validation("ReMa could not read the form on this page."))?;
    if answer.error.is_some() {
        return Err(AppError::validation(
            "ReMa could not read the form on this page.",
        ));
    }
    answer.fields.truncate(MAX_FIELDS);
    for f in &mut answer.fields {
        for s in [
            &mut f.tag,
            &mut f.input_type,
            &mut f.name,
            &mut f.id,
            &mut f.autocomplete,
            &mut f.label,
            &mut f.placeholder,
            &mut f.aria,
            &mut f.nearby,
        ] {
            cut(s);
        }
        f.options.truncate(300);
        f.options.iter_mut().for_each(cut);
    }
    answer.frames.truncate(10);
    Ok(answer)
}

// ── Deterministic mapping ───────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKey {
    FirstName,
    LastName,
    FullName,
    Email,
    Phone,
    Location,
    City,
    Country,
    Website,
    Portfolio,
    Github,
    Linkedin,
    Title,
    CurrentCompany,
}

impl FieldKey {
    pub fn label(self) -> &'static str {
        match self {
            Self::FirstName => "First name",
            Self::LastName => "Last name",
            Self::FullName => "Full name",
            Self::Email => "Email",
            Self::Phone => "Phone",
            Self::Location => "Location",
            Self::City => "City",
            Self::Country => "Country",
            Self::Website => "Website",
            Self::Portfolio => "Portfolio / resume website",
            Self::Github => "GitHub",
            Self::Linkedin => "LinkedIn",
            Self::Title => "Professional title",
            Self::CurrentCompany => "Current company",
        }
    }

    /// The profile's value for this field, if it has one.
    pub fn value(self, p: &Profile) -> Option<String> {
        let non_empty = |s: &str| {
            let s = s.trim();
            (!s.is_empty()).then(|| s.to_string())
        };
        let location_parts: Vec<&str> = p.location.split(',').map(str::trim).collect();
        match self {
            Self::FirstName => non_empty(&p.first_name),
            Self::LastName => non_empty(&p.last_name),
            Self::FullName => non_empty(&p.full_name()),
            Self::Email => non_empty(&p.email),
            Self::Phone => non_empty(&p.phone),
            Self::Location => non_empty(&p.location),
            Self::City => location_parts.first().and_then(|c| non_empty(c)),
            Self::Country => (location_parts.len() > 1)
                .then(|| location_parts.last().and_then(|c| non_empty(c)))
                .flatten(),
            Self::Website => non_empty(&p.website).or_else(|| non_empty(&p.resume_website)),
            Self::Portfolio => non_empty(&p.resume_website).or_else(|| non_empty(&p.website)),
            Self::Github => non_empty(&p.github),
            Self::Linkedin => non_empty(&p.linkedin),
            Self::Title => non_empty(&p.title),
            Self::CurrentCompany => p
                .experience
                .iter()
                .find(|e| e.current)
                .or(p.experience.first())
                .and_then(|e| non_empty(&e.company)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Classified {
    Profile(FieldKey),
    File(FileFieldKind),
    /// A question for the user (cover letter text, salary, …).
    Question,
    Ignore,
}

/// Lower-case words: "E-mail address:" → "e mail address".
fn words(value: &str) -> String {
    split_words(value, false)
}

/// Also splits machine names: "applicant[firstName]" → "applicant first name".
fn name_words(value: &str) -> String {
    split_words(value, true)
}

fn split_words(value: &str, split_camel_case: bool) -> String {
    let mut out = String::new();
    let mut prev_lower = false;
    for c in value.chars() {
        if split_camel_case && c.is_uppercase() && prev_lower {
            out.push(' ');
        }
        prev_lower = c.is_lowercase() || c.is_ascii_digit();
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else {
            out.push(' ');
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn has(text: &str, phrases: &[&str]) -> bool {
    let padded = format!(" {text} ");
    phrases.iter().any(|p| padded.contains(&format!(" {p} ")))
}

pub fn classify(f: &FieldInfo) -> Classified {
    let primary = format!(
        "{} {} {}",
        words(&format!("{} {} {}", f.label, f.aria, f.placeholder)),
        name_words(&f.name),
        name_words(&f.id)
    );
    let primary = primary.split_whitespace().collect::<Vec<_>>().join(" ");
    let nearby = words(&f.nearby);
    let labelled = [&f.label, &f.aria, &f.placeholder]
        .iter()
        .any(|s| !s.trim().is_empty());

    if f.input_type == "file" {
        let text = format!("{primary} {nearby}");
        return Classified::File(
            if has(
                &text,
                &[
                    "cover letter",
                    "coverletter",
                    "anschreiben",
                    "motivation",
                    "motivationsschreiben",
                ],
            ) {
                FileFieldKind::CoverLetter
            } else if has(
                &text,
                &["resume", "résumé", "cv", "lebenslauf", "curriculum"],
            ) {
                FileFieldKind::Resume
            } else {
                FileFieldKind::Other
            },
        );
    }

    // The standard `autocomplete` tokens are the most reliable signal.
    let token = f
        .autocomplete
        .split_whitespace()
        .last()
        .unwrap_or("")
        .to_lowercase();
    let by_token = match token.as_str() {
        "given-name" => Some(FieldKey::FirstName),
        "family-name" => Some(FieldKey::LastName),
        "name" => Some(FieldKey::FullName),
        "email" => Some(FieldKey::Email),
        "tel" | "tel-national" => Some(FieldKey::Phone),
        "address-level2" => Some(FieldKey::City),
        "country" | "country-name" => Some(FieldKey::Country),
        "url" => Some(FieldKey::Website),
        "organization-title" => Some(FieldKey::Title),
        "organization" => Some(FieldKey::CurrentCompany),
        _ => None,
    };
    if let Some(key) = by_token {
        return Classified::Profile(key);
    }

    // The field's own words first. Text near the field (often the previous
    // field's label) is only used when the field has no label at all.
    let found = match match_text(&primary, f) {
        Some(result) => Some(result),
        None if !labelled && !nearby.is_empty() => match_text(&nearby, f),
        None => None,
    };
    found.unwrap_or(if primary.is_empty() && nearby.is_empty() {
        Classified::Ignore
    } else {
        Classified::Question
    })
}

/// A profile field named by `text`, a question the profile must not
/// answer, or `None` when `text` says nothing either way.
fn match_text(text: &str, f: &FieldInfo) -> Option<Classified> {
    let typed = f.input_type == "email" || f.input_type == "tel";
    if text.is_empty() && !typed {
        return None;
    }
    // Things the profile does not answer, even when they mention a name or
    // a company ("How did you hear about us?", "Referrer's email").
    if has(
        text,
        &[
            "password",
            "captcha",
            "search",
            "coupon",
            "promo",
            "referral",
            "referred",
            "referrer",
            "reference",
            "salary",
            "gehalt",
            "compensation",
            "notice",
            "hear",
            "why",
            "how",
            "describe",
            "tell",
            "date of birth",
            "birthday",
            "street",
            "street address",
            "home address",
            "postal address",
            "address line",
            "zip",
            "postal",
            "postcode",
            "plz",
        ],
    ) {
        return Some(Classified::Question);
    }

    let key = if f.input_type == "email" || has(text, &["email", "e mail", "mail"]) {
        FieldKey::Email
    } else if f.input_type == "tel"
        || has(
            text,
            &[
                "phone",
                "telephone",
                "mobile",
                "tel",
                "telefon",
                "telefonnummer",
                "handy",
                "cell",
            ],
        )
    {
        FieldKey::Phone
    } else if has(text, &["linkedin", "linked in"]) {
        FieldKey::Linkedin
    } else if has(text, &["github", "git hub"]) {
        FieldKey::Github
    } else if has(
        text,
        &[
            "first name",
            "firstname",
            "given name",
            "fname",
            "vorname",
            "forename",
            "prenom",
            "prénom",
        ],
    ) {
        FieldKey::FirstName
    } else if has(
        text,
        &[
            "last name",
            "lastname",
            "surname",
            "family name",
            "lname",
            "nachname",
        ],
    ) {
        FieldKey::LastName
    } else if has(
        text,
        &[
            "full name",
            "your name",
            "applicant name",
            "candidate name",
            "legal name",
        ],
    ) || words(&f.label) == "name"
        || name_words(&f.name) == "name"
    {
        FieldKey::FullName
    } else if has(text, &["portfolio"]) {
        FieldKey::Portfolio
    } else if has(
        text,
        &[
            "website",
            "homepage",
            "personal site",
            "personal website",
            "blog",
            "url",
        ],
    ) {
        FieldKey::Website
    } else if has(
        text,
        &[
            "current title",
            "current job title",
            "headline",
            "professional title",
            "current position",
            "current role",
        ],
    ) {
        FieldKey::Title
    } else if has(
        text,
        &[
            "current company",
            "current employer",
            "employer",
            "current organization",
        ],
    ) {
        FieldKey::CurrentCompany
    } else if has(text, &["country", "land"]) {
        FieldKey::Country
    } else if has(text, &["city", "town", "stadt", "wohnort"]) {
        FieldKey::City
    } else if has(
        text,
        &["location", "based", "where do you live", "current location"],
    ) {
        FieldKey::Location
    } else {
        return None;
    };
    Some(Classified::Profile(key))
}

/// The select option matching a value, ignoring case.
fn option_for(options: &[String], value: &str) -> Option<usize> {
    let value = value.trim().to_lowercase();
    if value.is_empty() {
        return None;
    }
    options
        .iter()
        .position(|o| o.trim().to_lowercase() == value)
        .or_else(|| {
            options.iter().position(|o| {
                let o = o.trim().to_lowercase();
                o.len() >= 3 && (o.starts_with(&value) || value.starts_with(&o))
            })
        })
}

fn display_label(f: &FieldInfo) -> String {
    [&f.label, &f.aria, &f.placeholder, &f.nearby, &f.name]
        .into_iter()
        .find(|s| !s.trim().is_empty())
        .map(|s| s.trim_end_matches(['*', ':', ' ']).to_string())
        .unwrap_or_else(|| "Field".into())
}

/// Which values to set, and the summary for the user.
pub fn plan(
    fields: &[FieldInfo],
    frames: &[String],
    profile: &Profile,
) -> (Vec<Fill>, AutofillResult) {
    let mut fills = Vec::new();
    let mut result = AutofillResult::default();
    for f in fields {
        match classify(f) {
            Classified::Profile(key) => {
                let Some(value) = key.value(profile) else {
                    if !result.missing.iter().any(|m| m == key.label()) {
                        result.missing.push(key.label().to_string());
                    }
                    continue;
                };
                if !f.empty {
                    result.kept += 1;
                    continue;
                }
                let option = if f.tag == "select" {
                    match option_for(&f.options, &value) {
                        Some(i) => Some(i),
                        None => continue,
                    }
                } else {
                    None
                };
                fills.push(Fill {
                    index: f.i,
                    value,
                    option,
                });
                result.filled.push(FilledField {
                    label: display_label(f),
                    source: key.label().to_string(),
                });
            }
            Classified::File(kind) => result.files.push(FileField {
                field: f.i,
                label: display_label(f),
                kind,
            }),
            Classified::Question => {
                let label = display_label(f);
                if f.empty && result.questions.len() < 20 && !result.questions.contains(&label) {
                    result.questions.push(label);
                }
            }
            Classified::Ignore => {}
        }
    }
    for frame in frames {
        let web = frame.starts_with("https://") || frame.starts_with("http://");
        if web && !result.embedded_forms.contains(frame) && result.embedded_forms.len() < 5 {
            result.embedded_forms.push(frame.clone());
        }
    }
    (fills, result)
}

// ── Running in the page ─────────────────────────────────────────────

fn nonce() -> AppResult<String> {
    let mut bytes = [0u8; 12];
    getrandom::fill(&mut bytes).map_err(|e| AppError::internal(e.to_string()))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Runs a script in the browser page and returns its JSON answer.
async fn evaluate(app: &AppHandle, script: String) -> AppResult<String> {
    use tauri::Manager;

    let view = app
        .get_webview(LABEL)
        .ok_or_else(|| AppError::validation("Open a page in the browser first."))?;
    let (tx, rx) = tokio::sync::oneshot::channel::<String>();
    let tx = Mutex::new(Some(tx));
    view.eval_with_callback(script, move |answer| {
        if let Some(tx) = tx.lock().unwrap().take() {
            let _ = tx.send(answer);
        }
    })
    .map_err(|e| AppError::internal(e.to_string()))?;
    tokio::time::timeout(SCRIPT_TIMEOUT, rx)
        .await
        .map_err(|_| {
            AppError::validation("The page did not respond. Try again when it has loaded.")
        })?
        .map_err(|_| AppError::validation("The page did not respond."))
}

/// Fills the current page's form from the profile. Never submits.
pub async fn run(app: &AppHandle, state: &AppState) -> AppResult<AutofillResult> {
    let profile = state.db.call(|c| Ok(profile_repo::get(c)?.0))?;
    if profile.is_empty() {
        return Err(AppError::validation(
            "Your Profile is empty. Add your details on the Profile page first.",
        ));
    }
    let nonce = nonce()?;
    let scan = parse_scan(&evaluate(app, scan_script(&nonce)).await?)?;
    let (fills, mut result) = plan(&scan.fields, &scan.frames, &profile);

    if !fills.is_empty() {
        #[derive(Deserialize, Default)]
        #[serde(default)]
        struct FillAnswer {
            done: Vec<bool>,
        }
        let answer = evaluate(app, fill_script(&nonce, &fills)).await?;
        let done = serde_json::from_str::<FillAnswer>(&answer)
            .unwrap_or_default()
            .done;
        // Report only what the page actually accepted.
        let mut index = 0;
        result.filled.retain(|_| {
            let ok = done.get(index).copied().unwrap_or(false);
            index += 1;
            ok
        });
    }

    *state.browser.autofill.lock().unwrap() = Some(Session {
        nonce,
        url: scan.url,
        files: result.files.clone(),
    });
    Ok(result)
}

/// Attaches a profile document to an upload field found by the last run.
pub async fn attach(
    app: &AppHandle,
    state: &AppState,
    field: u32,
    document_id: i64,
) -> AppResult<()> {
    let (nonce, url) = {
        let session = state.browser.autofill.lock().unwrap();
        let session = session
            .as_ref()
            .ok_or_else(|| AppError::validation("Run ReMa Auto Fill on this page first."))?;
        if !session.files.iter().any(|f| f.field == field) {
            return Err(AppError::validation(
                "That upload field is no longer on the page.",
            ));
        }
        (session.nonce.clone(), session.url.clone())
    };
    if current_url(app).map(|u| u.to_string()) != Some(url) {
        return Err(AppError::validation(
            "The page changed. Run ReMa Auto Fill again.",
        ));
    }

    let (document, path) = profile_service::document_path(state, document_id)?;
    let bytes = std::fs::read(&path)?;
    if bytes.len() > MAX_ATTACH_BYTES {
        return Err(AppError::validation(
            "This file is too large to attach automatically. Use the website's upload button.",
        ));
    }
    let file_name = {
        let ext = document.format.extension();
        let base: String = document
            .name
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || " -_().".contains(c) {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        format!("{}.{ext}", base.trim())
    };
    let script = attach_script(
        &nonce,
        field,
        &file_name,
        document.format.mime(),
        &STANDARD.encode(&bytes),
    );

    #[derive(Deserialize, Default)]
    #[serde(default)]
    struct AttachAnswer {
        ok: bool,
    }
    let answer: AttachAnswer =
        serde_json::from_str(&evaluate(app, script).await?).unwrap_or_default();
    if !answer.ok {
        return Err(AppError::validation(
            "The website did not accept the file automatically. Use its upload button and \
             choose the file yourself (“Show file” opens its folder).",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::profile::Experience;

    fn field(i: u32, label: &str, name: &str, input_type: &str) -> FieldInfo {
        FieldInfo {
            i,
            tag: if input_type == "textarea" {
                "textarea".into()
            } else {
                "input".into()
            },
            input_type: input_type.into(),
            name: name.into(),
            label: label.into(),
            empty: true,
            ..FieldInfo::default()
        }
    }

    fn profile() -> Profile {
        Profile {
            first_name: "Ana".into(),
            last_name: "Tester".into(),
            email: "ana@example.com".into(),
            phone: "+43 660 1234567".into(),
            location: "Vienna, Austria".into(),
            title: "Data Engineer".into(),
            github: "https://github.com/ana".into(),
            linkedin: "https://linkedin.com/in/ana".into(),
            website: "https://ana.dev/".into(),
            experience: vec![Experience {
                company: "Globex".into(),
                current: true,
                ..Experience::default()
            }],
            ..Profile::default()
        }
    }

    #[test]
    fn recognizes_common_application_fields() {
        let cases: &[(FieldInfo, Classified)] = &[
            (
                field(0, "First name", "job_application[first_name]", "text"),
                Classified::Profile(FieldKey::FirstName),
            ),
            (
                field(0, "", "lastName", "text"),
                Classified::Profile(FieldKey::LastName),
            ),
            (
                field(0, "Vorname", "", "text"),
                Classified::Profile(FieldKey::FirstName),
            ),
            (
                field(0, "Nachname *", "", "text"),
                Classified::Profile(FieldKey::LastName),
            ),
            (
                field(0, "Name", "name", "text"),
                Classified::Profile(FieldKey::FullName),
            ),
            (
                field(0, "Email address", "", "text"),
                Classified::Profile(FieldKey::Email),
            ),
            (
                field(0, "", "", "email"),
                Classified::Profile(FieldKey::Email),
            ),
            (
                field(0, "Phone number", "", "text"),
                Classified::Profile(FieldKey::Phone),
            ),
            (
                field(0, "LinkedIn Profile URL", "urls[LinkedIn]", "text"),
                Classified::Profile(FieldKey::Linkedin),
            ),
            (
                field(0, "GitHub", "", "url"),
                Classified::Profile(FieldKey::Github),
            ),
            (
                field(0, "", "linkedInUrl", "url"),
                Classified::Profile(FieldKey::Linkedin),
            ),
            (
                field(0, "Website", "", "url"),
                Classified::Profile(FieldKey::Website),
            ),
            (
                field(0, "Portfolio link", "", "url"),
                Classified::Profile(FieldKey::Portfolio),
            ),
            (
                field(0, "Current title", "", "text"),
                Classified::Profile(FieldKey::Title),
            ),
            (
                field(0, "Current company", "org", "text"),
                Classified::Profile(FieldKey::CurrentCompany),
            ),
            (
                field(0, "Location (City)", "", "text"),
                Classified::Profile(FieldKey::City),
            ),
            (
                field(0, "Where are you based?", "", "text"),
                Classified::Profile(FieldKey::Location),
            ),
            (
                field(0, "Resume/CV", "resume", "file"),
                Classified::File(FileFieldKind::Resume),
            ),
            (
                field(0, "Cover Letter", "cover_letter", "file"),
                Classified::File(FileFieldKind::CoverLetter),
            ),
            // Questions the profile cannot answer stay with the user.
            (
                field(0, "Why are you interested in this role?", "q1", "textarea"),
                Classified::Question,
            ),
            (
                field(0, "What are your salary expectations?", "", "text"),
                Classified::Question,
            ),
            (
                field(0, "Referrer's email", "", "email"),
                Classified::Question,
            ),
            (
                field(0, "How did you hear about us?", "source", "text"),
                Classified::Question,
            ),
            (field(0, "Company name", "", "text"), Classified::Question),
            (
                field(0, "Street address", "address1", "text"),
                Classified::Question,
            ),
            (field(0, "", "", "text"), Classified::Ignore),
        ];
        for (f, expected) in cases {
            assert_eq!(classify(f), *expected, "{} / {}", f.label, f.name);
        }
        // Seen on a real form: a placeholder-only field right after "Phone".
        let mut city = field(0, "", "city", "text");
        city.placeholder = "City".into();
        city.nearby = "Phone".into();
        assert_eq!(classify(&city), Classified::Profile(FieldKey::City));
        // Nearby text still helps fields with no label of their own.
        let mut unlabeled = field(0, "", "field_17", "text");
        unlabeled.nearby = "Email address".into();
        assert_eq!(classify(&unlabeled), Classified::Profile(FieldKey::Email));

        let mut by_autocomplete = field(0, "Your details", "x1", "text");
        by_autocomplete.autocomplete = "section-a given-name".into();
        assert_eq!(
            classify(&by_autocomplete),
            Classified::Profile(FieldKey::FirstName)
        );
    }

    #[test]
    fn fills_only_empty_fields_and_reports_the_rest() {
        let mut kept = field(2, "Email", "email", "email");
        kept.empty = false;
        let mut country = field(4, "Country", "country", "select");
        country.tag = "select".into();
        country.options = vec!["Select…".into(), "Germany".into(), "Austria".into()];
        let fields = vec![
            field(0, "First name", "first_name", "text"),
            field(1, "Last name", "last_name", "text"),
            kept,
            field(3, "Why this role?", "why", "textarea"),
            country,
            field(5, "Resume", "resume", "file"),
            field(6, "Twitter", "twitter", "text"),
        ];
        let frames = vec!["https://boards.example-ats.com/embed/apply".to_string()];
        let (fills, result) = plan(&fields, &frames, &profile());

        assert_eq!(
            fills,
            vec![
                Fill {
                    index: 0,
                    value: "Ana".into(),
                    option: None
                },
                Fill {
                    index: 1,
                    value: "Tester".into(),
                    option: None
                },
                Fill {
                    index: 4,
                    value: "Austria".into(),
                    option: Some(2)
                },
            ]
        );
        assert_eq!(result.kept, 1, "the user's own value is not replaced");
        assert_eq!(result.filled.len(), 3);
        assert_eq!(result.files.len(), 1);
        assert_eq!(result.files[0].kind, FileFieldKind::Resume);
        assert_eq!(result.questions, ["Why this role?", "Twitter"]);
        assert_eq!(result.embedded_forms, frames);

        let (fills, result) = plan(
            &[field(0, "Phone", "phone", "tel")],
            &[],
            &Profile {
                first_name: "Ana".into(),
                ..Profile::default()
            },
        );
        assert!(fills.is_empty());
        assert_eq!(result.missing, ["Phone"]);
    }

    #[test]
    fn scripts_never_submit_or_click() {
        let scripts = [
            scan_script("abc"),
            fill_script(
                "abc",
                &[Fill {
                    index: 0,
                    value: "x\"</script>".into(),
                    option: None,
                }],
            ),
            attach_script("abc", 1, "cv\".pdf", "application/pdf", "AAAA"),
        ];
        for script in &scripts {
            let lower = script.to_lowercase();
            for forbidden in [
                "submit(",
                "requestsubmit",
                ".click(",
                "keydown",
                "keypress",
                "fetch(",
                "xmlhttprequest",
                "sendbeacon",
            ] {
                assert!(!lower.contains(forbidden), "{forbidden} in script");
            }
        }
        // Values are embedded as JSON literals, never as code.
        assert!(scripts[1].contains(r#""value":"x\"</script>""#));
        assert!(scripts[2].contains(r#"new File([bytes], "cv\".pdf""#));
    }

    #[test]
    fn rejects_oversized_or_malformed_page_answers() {
        assert!(parse_scan("not json").is_err());
        assert!(parse_scan(r#"{"error":"boom"}"#).is_err());
        assert!(parse_scan(&"x".repeat(MAX_ANSWER_BYTES + 1)).is_err());
        let long = format!(
            r#"{{"fields":[{{"i":0,"label":"{}"}}]}}"#,
            "a".repeat(5_000)
        );
        assert_eq!(parse_scan(&long).unwrap().fields[0].label.len(), MAX_TEXT);
    }
}
