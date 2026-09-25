//! The built-in browser workspace: one child webview (label `browser`) in
//! ReMa's window, showing real websites next to ReMa's own interface.
//!
//! Security boundary:
//! - The webview has no Tauri capability (`capabilities/default.json` grants
//!   permissions to the `main` webview only, and every app command is
//!   ACL-checked), and web pages are remote origins. A page cannot call any
//!   ReMa command, plugin or event.
//! - Navigation is limited to http(s) pages; ReMa's own origin, local files
//!   and custom schemes are blocked. `mailto:`/`tel:` open externally.
//! - Pop-ups open in the same view instead of new windows.
//! - Page data never flows back to ReMa except through Auto Fill, which the
//!   user starts explicitly (see `autofill`).

pub mod autofill;
#[cfg(target_os = "linux")]
mod linux;

use std::sync::{Arc, Mutex};

use tauri::{
    webview::{NewWindowResponse, PageLoadEvent, WebviewBuilder},
    AppHandle, LogicalPosition, LogicalSize, Manager, Rect, Url, Webview, WebviewUrl,
};
use tauri_plugin_opener::OpenerExt;

use crate::{
    error::{AppError, AppResult},
    models::browser::{BrowserBounds, BrowserStatus},
    state::AppState,
};

pub const LABEL: &str = "browser";

/// Runs in every page of the browser webview. It only changes where links
/// that would open a new window go (the same view), so one page fits the
/// single-view workspace on every platform. It exposes nothing.
const SINGLE_VIEW_SCRIPT: &str = r#"document.addEventListener("click", (event) => {
  const link = event.target && event.target.closest ? event.target.closest("a[target]") : null;
  if (link && link.href && link.target !== "_self" && !event.defaultPrevented) link.target = "_self";
}, true);"#;
const MAIN_WINDOW: &str = "main";

/// Browser state shared with services and commands.
#[derive(Clone, Default)]
pub struct BrowserContext {
    status: Arc<Mutex<BrowserStatus>>,
    pub(crate) autofill: Arc<Mutex<Option<autofill::Session>>>,
}

impl BrowserContext {
    pub fn status(&self) -> BrowserStatus {
        self.status.lock().unwrap().clone()
    }

    fn update(&self, state: &AppState, change: impl FnOnce(&mut BrowserStatus)) {
        let status = {
            let mut status = self.status.lock().unwrap();
            change(&mut status);
            status.clone()
        };
        state.events.browser_changed(status);
    }
}

/// Turns what the user typed or clicked into a web address.
pub fn parse_web_url(input: &str) -> AppResult<Url> {
    let input = input.trim();
    let invalid = || AppError::validation("Enter a web address, e.g. https://example.com");
    if input.is_empty() || input.len() > 8_000 {
        return Err(invalid());
    }
    let candidate = if input.contains("://") {
        input.to_string()
    } else {
        format!("https://{input}")
    };
    let url = Url::parse(&candidate).map_err(|_| invalid())?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(invalid());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(AppError::validation(
            "Addresses with a user name or password are not opened.",
        ));
    }
    Ok(url)
}

#[derive(Debug, PartialEq, Eq)]
pub enum Navigation {
    Allow,
    /// Hand to the operating system (mail, phone links).
    External,
    Block,
}

/// Where the browser webview may go. `app_origins` are ReMa's own
/// frontend origins, which must never load in the browser webview.
pub fn navigation_policy(url: &Url, app_origins: &[String]) -> Navigation {
    match url.scheme() {
        "http" | "https" => {
            let origin = url.origin().ascii_serialization();
            let own = app_origins.contains(&origin)
                || url
                    .host_str()
                    .is_some_and(|h| h == "tauri.localhost" || h.ends_with(".tauri.localhost"));
            if own {
                Navigation::Block
            } else {
                Navigation::Allow
            }
        }
        // Initial blank page and in-page objects (e.g. PDF previews).
        "about" | "blob" => Navigation::Allow,
        "mailto" | "tel" => Navigation::External,
        // file:, data:, javascript:, tauri:, ipc:, asset:, custom schemes…
        _ => Navigation::Block,
    }
}

fn app_origins(app: &AppHandle) -> Vec<String> {
    let mut origins = vec!["tauri://localhost".to_string()];
    if let Some(dev) = &app.config().build.dev_url {
        origins.push(dev.origin().ascii_serialization());
    }
    origins
}

fn webview(app: &AppHandle) -> Option<Webview> {
    app.get_webview(LABEL)
}

fn rect(bounds: BrowserBounds) -> Rect {
    Rect {
        position: LogicalPosition::new(bounds.x.max(0.0), bounds.y.max(0.0)).into(),
        size: LogicalSize::new(bounds.width.max(1.0), bounds.height.max(1.0)).into(),
    }
}

fn apply_bounds(view: &Webview, bounds: BrowserBounds) -> AppResult<()> {
    #[cfg(target_os = "linux")]
    {
        linux::set_bounds(view, bounds)
    }
    #[cfg(not(target_os = "linux"))]
    {
        view.set_bounds(rect(bounds))
            .map_err(|e| AppError::internal(e.to_string()))
    }
}

/// Opens `url` in the browser workspace, creating the webview if needed.
pub fn open(
    app: &AppHandle,
    state: &AppState,
    url: &str,
    bounds: BrowserBounds,
) -> AppResult<BrowserStatus> {
    let url = parse_web_url(url)?;
    if navigation_policy(&url, &app_origins(app)) != Navigation::Allow {
        return Err(AppError::validation(
            "This address cannot be opened in ReMa.",
        ));
    }
    *state.browser.autofill.lock().unwrap() = None;

    match webview(app) {
        Some(view) => {
            view.navigate(url.clone())
                .map_err(|e| AppError::internal(e.to_string()))?;
            apply_bounds(&view, bounds)?;
            let _ = view.show();
        }
        None => create(app, state, url.clone(), bounds)?,
    }
    state.browser.update(state, |s| {
        s.open = true;
        s.url = Some(url.to_string());
        s.loading = true;
    });
    Ok(state.browser.status())
}

fn create(app: &AppHandle, state: &AppState, url: Url, bounds: BrowserBounds) -> AppResult<()> {
    let window = app
        .get_window(MAIN_WINDOW)
        .ok_or_else(|| AppError::internal("the main window is not available"))?;
    let origins = app_origins(app);

    let opener = app.clone();
    let on_navigation = move |url: &Url| match navigation_policy(url, &origins) {
        Navigation::Allow => true,
        Navigation::External => {
            let _ = opener.opener().open_url(url.as_str(), None::<&str>);
            false
        }
        Navigation::Block => false,
    };

    // Pop-ups (target=_blank, window.open) replace the current page. The
    // navigation is queued: WebKit ignores loads started inside its
    // new-window callback.
    let handle = app.clone();
    let popup_origins = app_origins(app);
    let on_new_window = move |url: Url, _features| {
        if navigation_policy(&url, &popup_origins) == Navigation::Allow {
            let handle = handle.clone();
            tauri::async_runtime::spawn(async move {
                if let Some(view) = handle.get_webview(LABEL) {
                    let _ = view.navigate(url);
                }
            });
        }
        NewWindowResponse::Deny
    };

    let page_state = state.clone();
    let on_page_load = move |_view: Webview, payload: tauri::webview::PageLoadPayload<'_>| {
        let url = payload.url().to_string();
        let loading = payload.event() == PageLoadEvent::Started;
        if loading {
            // A new page: forms from the old page are gone.
            *page_state.browser.autofill.lock().unwrap() = None;
        }
        page_state.browser.update(&page_state, |s| {
            s.url = Some(url);
            s.loading = loading;
            if loading {
                s.title = None;
            }
        });
    };

    let title_state = state.clone();
    let on_title = move |_view: Webview, title: String| {
        let title: String = title.chars().take(300).collect();
        title_state.browser.update(&title_state, |s| {
            s.title = (!title.trim().is_empty()).then_some(title);
        });
    };

    let builder = WebviewBuilder::new(LABEL, WebviewUrl::External(url))
        .on_navigation(on_navigation)
        .on_new_window(on_new_window)
        .on_page_load(on_page_load)
        .on_document_title_changed(on_title)
        .initialization_script(SINGLE_VIEW_SCRIPT)
        .focused(true);

    let area = rect(bounds);
    let view = window
        .add_child(builder, area.position, area.size)
        .map_err(|e| AppError::internal(format!("could not open the browser: {e}")))?;
    #[cfg(target_os = "linux")]
    linux::embed(&view, bounds)?;
    #[cfg(not(target_os = "linux"))]
    let _ = view;
    Ok(())
}

pub fn set_bounds(app: &AppHandle, bounds: BrowserBounds) -> AppResult<()> {
    match webview(app) {
        Some(view) => apply_bounds(&view, bounds),
        None => Ok(()),
    }
}

/// Hides the page (collapsed panel, or a ReMa dialog on top) or shows it.
pub fn set_visible(app: &AppHandle, visible: bool) -> AppResult<()> {
    if let Some(view) = webview(app) {
        let result = if visible { view.show() } else { view.hide() };
        result.map_err(|e| AppError::internal(e.to_string()))?;
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
pub enum HistoryStep {
    Back,
    Forward,
}

pub fn go(app: &AppHandle, step: HistoryStep) -> AppResult<()> {
    let view = webview(app).ok_or_else(|| AppError::validation("The browser is closed."))?;
    let script = match step {
        HistoryStep::Back => "history.back()",
        HistoryStep::Forward => "history.forward()",
    };
    view.eval(script)
        .map_err(|e| AppError::internal(e.to_string()))
}

pub fn reload(app: &AppHandle) -> AppResult<()> {
    let view = webview(app).ok_or_else(|| AppError::validation("The browser is closed."))?;
    view.reload().map_err(|e| AppError::internal(e.to_string()))
}

/// Closes the webview. Cookies and sign-ins stay in the browser's store.
pub fn close(app: &AppHandle, state: &AppState) -> AppResult<()> {
    if let Some(view) = webview(app) {
        view.close()
            .map_err(|e| AppError::internal(e.to_string()))?;
    }
    *state.browser.autofill.lock().unwrap() = None;
    state.browser.update(state, |s| {
        s.open = false;
        s.loading = false;
    });
    Ok(())
}

/// The page currently shown, if any.
pub fn current_url(app: &AppHandle) -> Option<Url> {
    webview(app).and_then(|v| v.url().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_typed_and_clicked_addresses() {
        assert_eq!(
            parse_web_url(" jobs.example.com/apply ").unwrap().as_str(),
            "https://jobs.example.com/apply"
        );
        assert_eq!(
            parse_web_url("http://example.com").unwrap().as_str(),
            "http://example.com/"
        );
        for bad in [
            "",
            "javascript:alert(1)",
            "file:///etc/passwd",
            "ftp://example.com",
            "https://user:secret@example.com",
            "data:text/html,hi",
        ] {
            assert!(parse_web_url(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn keeps_the_browser_on_the_web() {
        let own = vec![
            "tauri://localhost".to_string(),
            "http://localhost:1420".to_string(),
        ];
        let policy = |url: &str| navigation_policy(&Url::parse(url).unwrap(), &own);
        assert_eq!(policy("https://jobs.example.com/apply"), Navigation::Allow);
        assert_eq!(policy("http://localhost:8080/"), Navigation::Allow);
        assert_eq!(policy("about:blank"), Navigation::Allow);
        assert_eq!(policy("mailto:hr@example.com"), Navigation::External);
        assert_eq!(policy("tel:+43123"), Navigation::External);
        // ReMa's own interface never loads inside the browser.
        assert_eq!(policy("http://localhost:1420/"), Navigation::Block);
        assert_eq!(policy("tauri://localhost/"), Navigation::Block);
        assert_eq!(policy("http://tauri.localhost/"), Navigation::Block);
        for blocked in [
            "file:///home/user/.ssh/id_rsa",
            "data:text/html,<script>1</script>",
            "javascript:alert(1)",
            "ipc://localhost/get_profile",
            "asset://localhost/x",
        ] {
            assert_eq!(policy(blocked), Navigation::Block, "{blocked}");
        }
    }
}
