//! App registrations entered in Settings (Settings → Connectors → Set up).
//!
//! ReMa signs users in with its own Google, Microsoft and LinkedIn app
//! registrations (public OAuth clients). Official builds carry them
//! (`src-tauri/connectors.toml`, see `build_config`); a copy built without
//! one lets the person who runs it enter the registration once, in the app,
//! instead of at a terminal before building. What is entered completes what
//! the build lacks and never overrides it.
//!
//! Where it lives: the public values (client IDs, the Google app's
//! publishing status, LinkedIn's approved scopes) in `<data dir>/connectors.toml`,
//! with the same tables and keys as `src-tauri/connectors.toml`, so the same
//! file can also be written by hand; the optional Google Desktop client
//! secret in the system keychain, never in the file (a file written by hand
//! may still carry one, and is honored).

use std::{fmt, path::Path};

use crate::{
    db::connectors as repo,
    error::{AppError, AppResult},
    models::connectors::{AppRegistration, AppRegistrationInput, ProviderId, RegistrationSource},
    state::AppState,
};

use super::{build_config, diag, linkedin, oauth::OAuthApp, RUNTIME_CONFIG_FILE};

/// Keychain account of the Google Desktop client secret entered in Settings.
pub const GOOGLE_SECRET_ACCOUNT: &str = "connector-app:google:client_secret";

/// One provider's registration as entered in Settings (or written into the
/// data folder's `connectors.toml`).
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Entered {
    pub client_id: String,
    /// Google only: kept in the keychain (or a hand-written file).
    pub client_secret: Option<String>,
    /// Google: "testing", "production" or "" (not said).
    pub publishing_status: String,
    /// LinkedIn: approved restricted scopes, space-separated.
    pub approved_scopes: String,
}

impl fmt::Debug for Entered {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Entered")
            .field("client_id", &self.client_id)
            .field(
                "client_secret",
                &self.client_secret.as_ref().map(|_| "<redacted>"),
            )
            .field("publishing_status", &self.publishing_status)
            .field("approved_scopes", &self.approved_scopes)
            .finish()
    }
}

impl Entered {
    fn app(&self) -> OAuthApp {
        OAuthApp {
            client_id: self.client_id.clone(),
            client_secret: self.client_secret.clone().filter(|s| !s.trim().is_empty()),
        }
    }
}

/// Every registration entered for this copy of ReMa.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnteredRegistrations {
    pub google: Option<Entered>,
    pub microsoft: Option<Entered>,
    pub linkedin: Option<Entered>,
}

impl EnteredRegistrations {
    pub fn get(&self, provider: ProviderId) -> Option<&Entered> {
        match provider {
            ProviderId::Google => self.google.as_ref(),
            ProviderId::Microsoft => self.microsoft.as_ref(),
            ProviderId::Linkedin => self.linkedin.as_ref(),
            ProviderId::Xing => None,
        }
    }

    fn set(&mut self, provider: ProviderId, entered: Option<Entered>) {
        match provider {
            ProviderId::Google => self.google = entered,
            ProviderId::Microsoft => self.microsoft = entered,
            ProviderId::Linkedin => self.linkedin = entered,
            ProviderId::Xing => {}
        }
    }

    pub fn app(&self, provider: ProviderId) -> Option<OAuthApp> {
        self.get(provider).map(Entered::app)
    }

    /// What the entered Google registration says about the app's
    /// publishing status.
    pub fn google_in_testing(&self) -> Option<bool> {
        match self.google.as_ref()?.publishing_status.as_str() {
            "testing" => Some(true),
            "production" => Some(false),
            _ => None,
        }
    }

    /// The restricted LinkedIn scopes the entered registration lists.
    pub fn linkedin_scopes(&self) -> Vec<String> {
        self.linkedin
            .as_ref()
            .map(|l| linkedin::known(&l.approved_scopes))
            .unwrap_or_default()
    }

    /// Reads `<data dir>/connectors.toml`. No file means nothing entered;
    /// a malformed file is refused by key, never by value.
    pub fn read(data_dir: &Path) -> Result<Self, Vec<String>> {
        let path = data_dir.join(RUNTIME_CONFIG_FILE);
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Ok(Self::default());
        };
        Self::from_toml(&text)
    }

    /// Registrations from the text of a `connectors.toml` (checked like the
    /// build's; errors name keys, never values).
    pub fn from_toml(text: &str) -> Result<Self, Vec<String>> {
        let table: toml::Table = text
            .parse()
            .map_err(|e: toml::de::Error| vec![format!("not valid TOML: {e}")])?;
        let file = |key: build_config::Key| {
            table
                .get(key.table)
                .and_then(|t| t.get(key.name))
                .and_then(|v| v.as_str())
                .map(str::to_string)
        };
        let config = build_config::resolve(&file, &|_| None, false)?;
        Ok(Self {
            google: config.google_client_id.map(|client_id| Entered {
                client_id,
                client_secret: config.google_client_secret.filter(|v| !v.trim().is_empty()),
                publishing_status: config.google_publishing_status,
                approved_scopes: String::new(),
            }),
            microsoft: config.microsoft_client_id.map(|client_id| Entered {
                client_id,
                ..Entered::default()
            }),
            linkedin: config.linkedin_client_id.map(|client_id| Entered {
                client_id,
                approved_scopes: config.linkedin_approved_scopes,
                ..Entered::default()
            }),
        })
    }

    /// The file's text: public values only, never a secret.
    pub fn to_toml(&self) -> String {
        let mut out = String::from(
            "# ReMa's app registrations entered in Settings → Connectors (public OAuth\n\
             # client IDs; no secret is ever written here). Same keys as\n\
             # src-tauri/connectors.toml. Written by ReMa; editing by hand is fine.\n",
        );
        let quote = |v: &str| toml::Value::String(v.to_string()).to_string();
        if let Some(google) = &self.google {
            out.push_str("\n[google]\n");
            out.push_str(&format!(
                "desktop_client_id = {}\n",
                quote(&google.client_id)
            ));
            if !google.publishing_status.is_empty() {
                out.push_str(&format!(
                    "publishing_status = {}\n",
                    quote(&google.publishing_status)
                ));
            }
        }
        if let Some(microsoft) = &self.microsoft {
            out.push_str("\n[microsoft]\n");
            out.push_str(&format!(
                "public_client_id = {}\n",
                quote(&microsoft.client_id)
            ));
        }
        if let Some(linkedin) = &self.linkedin {
            out.push_str("\n[linkedin]\n");
            out.push_str(&format!("client_id = {}\n", quote(&linkedin.client_id)));
            if !linkedin.approved_scopes.is_empty() {
                out.push_str(&format!(
                    "approved_scopes = {}\n",
                    quote(&linkedin.approved_scopes)
                ));
            }
        }
        out
    }

    /// Writes `<data dir>/connectors.toml` in one step (a temporary file,
    /// then a rename), or removes it when nothing is entered.
    pub fn write(&self, data_dir: &Path) -> AppResult<()> {
        let path = data_dir.join(RUNTIME_CONFIG_FILE);
        let failed = |what: &str, e: std::io::Error| {
            AppError::internal(format!("could not {what} {}: {e}", path.display()))
        };
        if self.google.is_none() && self.microsoft.is_none() && self.linkedin.is_none() {
            return match std::fs::remove_file(&path) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(failed("remove", e)),
            };
        }
        std::fs::create_dir_all(data_dir).map_err(|e| failed("create the folder of", e))?;
        let temp = data_dir.join(format!("{RUNTIME_CONFIG_FILE}.{}.tmp", std::process::id()));
        std::fs::write(&temp, self.to_toml()).map_err(|e| failed("write", e))?;
        std::fs::rename(&temp, &path).map_err(|e| {
            let _ = std::fs::remove_file(&temp);
            failed("replace", e)
        })
    }
}

/// The name of what a provider's registration needs, for messages.
fn client_id_label(provider: ProviderId) -> &'static str {
    match provider {
        ProviderId::Google => "the client ID of a Google OAuth client of type Desktop app",
        ProviderId::Microsoft => {
            "the application (client) ID of a Microsoft Entra app registration"
        }
        ProviderId::Linkedin => "the client ID of a LinkedIn app",
        ProviderId::Xing => "",
    }
}

fn check_provider(state: &AppState, provider: ProviderId) -> AppResult<()> {
    if provider == ProviderId::Xing {
        return Err(AppError::validation(
            "XING offers no sign-in for desktop apps, so there is nothing to set up.",
        ));
    }
    if state.connectors.build_app(provider).is_some() {
        return Err(AppError::validation(format!(
            "This copy of ReMa already includes the {} registration; there is nothing to enter.",
            provider.name()
        )));
    }
    Ok(())
}

/// Checks the entered values (formats, never their meaning at the provider:
/// that shows on the first sign-in).
fn checked(
    provider: ProviderId,
    input: &AppRegistrationInput,
    current: Option<&Entered>,
) -> AppResult<Entered> {
    let client_id = input.client_id.trim();
    if client_id.is_empty() {
        return Err(AppError::validation(format!(
            "Enter {}.",
            client_id_label(provider)
        )));
    }
    let mut entered = Entered {
        client_id: client_id.to_string(),
        ..Entered::default()
    };
    match provider {
        ProviderId::Google => {
            if !build_config::valid_google_client_id(client_id) {
                return Err(AppError::validation(
                    "The Google client ID should end with .apps.googleusercontent.com (an OAuth \
                     client of type Desktop app).",
                ));
            }
            entered.client_secret = match input.client_secret.as_deref().map(str::trim) {
                None => current.and_then(|c| c.client_secret.clone()),
                Some("") => None,
                Some(secret) => {
                    if !build_config::valid_google_client_secret(secret) {
                        return Err(AppError::validation(
                            "The client secret looks wrong: 8 to 128 characters without spaces \
                             or quotes.",
                        ));
                    }
                    Some(secret.to_string())
                }
            };
            let status = input.publishing_status.trim().to_ascii_lowercase();
            if !["", "testing", "production"].contains(&status.as_str()) {
                return Err(AppError::validation(
                    "Publishing status must be Testing, Production or not set.",
                ));
            }
            entered.publishing_status = status;
        }
        ProviderId::Microsoft => {
            if !build_config::valid_guid(client_id) {
                return Err(AppError::validation(
                    "The Microsoft application (client) ID is a GUID, like \
                     12345678-1234-1234-1234-123456789abc.",
                ));
            }
        }
        ProviderId::Linkedin => {
            if !build_config::valid_linkedin_client_id(client_id) {
                return Err(AppError::validation(
                    "The LinkedIn client ID is 8 to 64 letters and digits.",
                ));
            }
            let scopes: Vec<&str> = input.approved_scopes.split_whitespace().collect();
            if let Some(unknown) = scopes
                .iter()
                .find(|s| !build_config::LINKEDIN_KNOWN_SCOPES.contains(s))
            {
                return Err(AppError::validation(format!(
                    "ReMa does not use the LinkedIn scope \"{unknown}\"; only {} is recognised.",
                    build_config::LINKEDIN_KNOWN_SCOPES.join(", ")
                )));
            }
            entered.approved_scopes = scopes.join(" ");
        }
        ProviderId::Xing => unreachable!("checked by check_provider"),
    }
    Ok(entered)
}

/// Enters (or changes) a provider's registration: checked, written to the
/// data folder (the Google secret to the keychain), and in use at once. A
/// connected account keeps its registration: its sign-in belongs to that
/// client ID, so the account is disconnected first.
pub async fn set(
    state: &AppState,
    provider: ProviderId,
    input: AppRegistrationInput,
) -> AppResult<()> {
    check_provider(state, provider)?;
    let current = state.connectors.entered();
    let entered = checked(provider, &input, current.get(provider))?;
    // An account connected with the registration in use keeps it: its
    // sign-in belongs to that client ID. (An account left over from a copy
    // of ReMa that carried a registration may be given one again; a grant
    // the new client cannot renew asks for a reconnect, as usual.)
    let connected = state.db.call(|c| repo::account(c, provider))?.is_some();
    if connected
        && current
            .get(provider)
            .is_some_and(|c| c.client_id != entered.client_id)
    {
        return Err(AppError::validation(format!(
            "Disconnect the {} account first: its sign-in belongs to the registration in use.",
            provider.name()
        )));
    }
    let mut next = current.clone();
    next.set(provider, Some(entered.clone()));
    // The file never carries the secret, the keychain does.
    let mut public = next.clone();
    if let Some(google) = public.google.as_mut() {
        google.client_secret = None;
    }
    public.write(&state.data_dir)?;
    if provider == ProviderId::Google {
        match &entered.client_secret {
            Some(secret) => state.vault.set_text(GOOGLE_SECRET_ACCOUNT, secret).await?,
            None => state.vault.delete_text(GOOGLE_SECRET_ACCOUNT).await?,
        }
    }
    state.connectors.set_entered(next);
    state.connectors.clear_failure(provider);
    diag(format!(
        "[connector] registration provider={} source=settings secret={}",
        provider.as_str(),
        if entered.client_secret.is_some() {
            "stored"
        } else {
            "none"
        }
    ));
    state.events.connectors_changed();
    Ok(())
}

/// Removes a registration entered in Settings. An account connected with it
/// is disconnected first.
pub async fn remove(state: &AppState, provider: ProviderId) -> AppResult<()> {
    check_provider(state, provider)?;
    if state.db.call(|c| repo::account(c, provider))?.is_some() {
        return Err(AppError::validation(format!(
            "Disconnect the {} account first.",
            provider.name()
        )));
    }
    let mut next = state.connectors.entered();
    next.set(provider, None);
    let mut public = next.clone();
    if let Some(google) = public.google.as_mut() {
        google.client_secret = None;
    }
    public.write(&state.data_dir)?;
    if provider == ProviderId::Google {
        state.vault.delete_text(GOOGLE_SECRET_ACCOUNT).await?;
    }
    state.connectors.set_entered(next);
    state.connectors.clear_failure(provider);
    diag(format!(
        "[connector] registration provider={} source=none",
        provider.as_str()
    ));
    state.events.connectors_changed();
    Ok(())
}

/// At start: the Google secret entered in Settings (keychain) completes the
/// public values read from the data folder. A keychain that does not answer
/// leaves the registration as it is; the first sign-in then says what
/// Google refused.
pub async fn load_secret(state: &AppState) {
    let entered = state.connectors.entered();
    let Some(google) = entered.google.as_ref() else {
        return;
    };
    if google.client_secret.is_some() {
        return;
    }
    match state.vault.get_text(GOOGLE_SECRET_ACCOUNT).await {
        Ok(Some(secret)) if !secret.trim().is_empty() => {
            let mut next = entered.clone();
            if let Some(google) = next.google.as_mut() {
                google.client_secret = Some(secret);
            }
            state.connectors.set_entered(next);
            diag("[connector] registration provider=google secret=loaded".to_string());
        }
        Ok(_) => {}
        Err(error) => diag(format!(
            "[connector] registration provider=google secret=unreadable: {error}"
        )),
    }
}

/// The registrations as Settings shows them (Google, Microsoft, LinkedIn).
pub fn overview(state: &AppState) -> Vec<AppRegistration> {
    [
        ProviderId::Google,
        ProviderId::Microsoft,
        ProviderId::Linkedin,
    ]
    .into_iter()
    .map(|provider| state.connectors.registration(provider))
    .collect()
}

impl super::ConnectorsContext {
    /// A provider's registration as Settings shows it.
    pub fn registration(&self, provider: ProviderId) -> AppRegistration {
        if let Some(app) = self.build_app(provider) {
            return AppRegistration {
                provider,
                source: RegistrationSource::Build,
                client_id: Some(app.client_id),
                client_secret_set: app.client_secret.is_some(),
                publishing_status: match provider {
                    ProviderId::Google => super::config::GOOGLE_PUBLISHING_STATUS.to_string(),
                    _ => String::new(),
                },
                approved_scopes: match provider {
                    ProviderId::Linkedin => linkedin::approved_scopes().join(" "),
                    _ => String::new(),
                },
                editable: false,
            };
        }
        let entered = self.entered();
        match entered.get(provider) {
            Some(e) => AppRegistration {
                provider,
                source: RegistrationSource::Settings,
                client_id: Some(e.client_id.clone()),
                client_secret_set: e.client_secret.is_some(),
                publishing_status: e.publishing_status.clone(),
                approved_scopes: e.approved_scopes.clone(),
                editable: true,
            },
            None => AppRegistration {
                provider,
                source: RegistrationSource::None,
                client_id: None,
                client_secret_set: false,
                publishing_status: String::new(),
                approved_scopes: String::new(),
                editable: provider != ProviderId::Xing,
            },
        }
    }
}
