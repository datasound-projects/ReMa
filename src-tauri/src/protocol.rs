//! `rema-doc://` — Profile document files for ReMa's own interface.
//!
//! The document viewer loads a stored file by id
//! (`rema-doc://localhost/document/<id>`, or
//! `http://rema-doc.localhost/document/<id>` on Windows), so the interface
//! never handles file paths and large files skip JSON. Only ReMa's main
//! webview is served: web pages in the built-in browser get nothing.

use tauri::http::{header, Response, StatusCode};

use crate::{error::AppError, services::profile, state::AppState};

pub const SCHEME: &str = "rema-doc";

/// The webview that may read documents.
const MAIN_WEBVIEW: &str = "main";

fn plain(status: StatusCode, message: &str) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .header(header::CACHE_CONTROL, "no-store")
        .body(message.as_bytes().to_vec())
        .expect("static response")
}

/// Answers one request. `origin` is echoed for ReMa's own pages so the
/// viewer can `fetch` the bytes.
pub fn respond(
    state: Option<&AppState>,
    webview: &str,
    path: &str,
    origin: Option<&str>,
) -> Response<Vec<u8>> {
    if webview != MAIN_WEBVIEW {
        return plain(StatusCode::FORBIDDEN, "Forbidden");
    }
    let Some(state) = state else {
        return plain(StatusCode::SERVICE_UNAVAILABLE, "ReMa is starting");
    };
    let Some(id) = path
        .strip_prefix("/document/")
        .and_then(|id| id.parse::<i64>().ok())
    else {
        return plain(StatusCode::NOT_FOUND, "Not found");
    };
    match profile::document_content(state, id) {
        Ok((document, bytes)) => {
            let mut response = Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, document.format.mime())
                .header(header::CACHE_CONTROL, "no-store")
                .header("X-Content-Type-Options", "nosniff")
                // Opened directly, a document can never run anything.
                .header(
                    header::CONTENT_SECURITY_POLICY,
                    "sandbox; default-src 'none'",
                );
            if let Some(origin) = origin {
                response = response.header(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin);
            }
            response.body(bytes).expect("document response")
        }
        Err(AppError::NotFound(_)) => plain(StatusCode::NOT_FOUND, "Not found"),
        Err(_) => plain(StatusCode::INTERNAL_SERVER_ERROR, "Unreadable"),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::{llm::fake::FakeLanguageModel, services::documents, state::testing};

    #[tokio::test]
    async fn serves_documents_to_the_main_webview_only() {
        let (state, _) = testing::state(Arc::new(FakeLanguageModel::replying(&[])));
        let source = state.data_dir.join("cv.pdf");
        let pdf = documents::samples::pdf(&["Ana"]);
        std::fs::write(&source, &pdf).unwrap();
        let doc = profile::add_document(&state, &source, None).await.unwrap();
        let path = format!("/document/{}", doc.id);

        let ok = respond(Some(&state), "main", &path, Some("http://localhost:1420"));
        assert_eq!(ok.status(), StatusCode::OK);
        assert_eq!(ok.headers()[header::CONTENT_TYPE], "application/pdf");
        assert_eq!(ok.body(), &pdf);

        // Pages in the built-in browser cannot read documents.
        assert_eq!(
            respond(Some(&state), "browser", &path, None).status(),
            StatusCode::FORBIDDEN
        );
        for bad in [
            "/document/999",
            "/document/../../etc/passwd",
            "/other/1",
            "/",
        ] {
            assert_eq!(
                respond(Some(&state), "main", bad, None).status(),
                StatusCode::NOT_FOUND,
                "{bad}"
            );
        }
    }
}
