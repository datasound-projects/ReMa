use crate::{
    db::{providers as settings_repo, Database},
    error::AppResult,
    models::system::{AppStatus, Appearance, BackendStatus},
    state::AppInfo,
};

const APPEARANCE_KEY: &str = "appearance.theme";

/// Reports the current status of the application core.
pub fn app_status(info: &AppInfo) -> AppStatus {
    AppStatus {
        status: BackendStatus::Ready,
        app: info.name.clone(),
        version: info.version.clone(),
    }
}

/// The saved color theme (light when never chosen).
pub fn appearance(db: &Database) -> AppResult<Appearance> {
    db.call(|conn| {
        Ok(settings_repo::get_setting(conn, APPEARANCE_KEY)?
            .and_then(|v| Appearance::parse(&v))
            .unwrap_or_default())
    })
}

/// Remembers the color theme, so the window opens in it next time.
pub fn save_appearance(db: &Database, appearance: Appearance) -> AppResult<()> {
    db.call(|conn| settings_repo::set_setting(conn, APPEARANCE_KEY, appearance.as_str()))
}

/// Only web and mail links may be opened from rendered content.
pub fn is_safe_external_url(url: &str) -> bool {
    reqwest::Url::parse(url).is_ok_and(|u| matches!(u.scheme(), "http" | "https" | "mailto"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info() -> AppInfo {
        AppInfo {
            name: "ReMa".into(),
            version: "0.1.0".into(),
        }
    }

    #[test]
    fn reports_ready_with_app_identity() {
        let status = app_status(&info());

        assert_eq!(
            status,
            AppStatus {
                status: BackendStatus::Ready,
                app: "ReMa".into(),
                version: "0.1.0".into(),
            }
        );
    }

    #[test]
    fn serializes_to_the_frontend_contract() {
        let json = serde_json::to_value(app_status(&info())).unwrap();

        assert_eq!(
            json,
            serde_json::json!({ "status": "ready", "app": "ReMa", "version": "0.1.0" })
        );
    }

    #[test]
    fn remembers_the_appearance() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(appearance(&db).unwrap(), Appearance::Light);
        save_appearance(&db, Appearance::Dark).unwrap();
        assert_eq!(appearance(&db).unwrap(), Appearance::Dark);
        assert_eq!(
            serde_json::to_value(Appearance::Dark).unwrap(),
            serde_json::json!("dark")
        );
    }

    #[test]
    fn allows_only_web_and_mail_links() {
        assert!(is_safe_external_url("https://example.com/a?b=c"));
        assert!(is_safe_external_url("mailto:hi@example.com"));
        assert!(!is_safe_external_url("javascript:alert(1)"));
        assert!(!is_safe_external_url("file:///etc/passwd"));
        assert!(!is_safe_external_url("not a url"));
    }
}
