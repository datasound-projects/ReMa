//! The local end of a browser sign-in: a one-shot HTTP listener on
//! 127.0.0.1 that receives the provider's redirect (RFC 8252 loopback
//! redirect for native apps).

use std::time::Duration;

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use tokio_util::sync::CancellationToken;

use crate::error::{AppError, AppResult};

/// The pages shown in the browser once the redirect arrived.
pub struct Pages<'a> {
    /// Product name for the messages, e.g. "Google" or an MCP server name.
    pub service: &'a str,
}

fn page(title: &str, text: &str) -> String {
    let escape = |s: &str| {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    };
    format!(
        "<!doctype html><meta charset=utf-8><title>ReMa</title>\
         <body style=\"font-family:-apple-system,Segoe UI,sans-serif;text-align:center;padding:64px\">\
         <h2>{}</h2><p>{}</p>",
        escape(title),
        escape(text)
    )
}

/// Waits for a request whose target `is_redirect` accepts and returns that
/// target (path and query). Other requests (a favicon) get a 404.
pub async fn receive(
    listener: TcpListener,
    is_redirect: impl Fn(&str) -> bool,
    succeeded: impl Fn(&str) -> bool,
    pages: Pages<'_>,
    cancel: &CancellationToken,
    timeout: Duration,
) -> AppResult<String> {
    let deadline = tokio::time::sleep(timeout);
    tokio::pin!(deadline);
    loop {
        let (mut socket, _) = tokio::select! {
            _ = cancel.cancelled() => {
                return Err(AppError::validation(format!("{} sign-in was cancelled.", pages.service)))
            }
            _ = &mut deadline => {
                return Err(AppError::validation(format!("{} sign-in timed out. Try again.", pages.service)))
            }
            accepted = listener.accept() => accepted?,
        };
        let mut buf = vec![0u8; 8192];
        let mut len = 0;
        // Read until the end of the request headers (the query carries all we need).
        while len < buf.len() {
            let n = tokio::time::timeout(Duration::from_secs(5), socket.read(&mut buf[len..]))
                .await
                .unwrap_or(Ok(0))?;
            if n == 0 {
                break;
            }
            len += n;
            if buf[..len].windows(4).any(|w| w == b"\r\n\r\n") {
                break;
            }
        }
        let request = String::from_utf8_lossy(&buf[..len]);
        let target = request
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .unwrap_or("/")
            .to_string();

        let accepted = is_redirect(&target);
        let (status, body) = if !accepted {
            ("404 Not Found", String::new())
        } else if succeeded(&target) {
            (
                "200 OK",
                page(
                    &format!("ReMa is connected to {}", pages.service),
                    "You can close this tab and return to ReMa.",
                ),
            )
        } else {
            (
                "200 OK",
                page(
                    &format!("{} sign-in was not completed", pages.service),
                    "You can close this tab and try again in ReMa.",
                ),
            )
        };
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = socket.write_all(response.as_bytes()).await;
        let _ = socket.shutdown().await;
        if accepted {
            return Ok(target);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn returns_the_redirect_and_ignores_other_requests() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let cancel = CancellationToken::new();
        let waiting = tokio::spawn(async move {
            receive(
                listener,
                |t| t.starts_with("/callback"),
                |t| t.contains("code="),
                Pages {
                    service: "Jobs <MCP>",
                },
                &cancel,
                Duration::from_secs(10),
            )
            .await
        });
        let get = |path: &'static str| async move {
            let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port))
                .await
                .unwrap();
            s.write_all(format!("GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes())
                .await
                .unwrap();
            let mut out = String::new();
            s.read_to_string(&mut out).await.unwrap();
            out
        };
        assert!(get("/favicon.ico").await.starts_with("HTTP/1.1 404"));
        let page = get("/callback?code=abc&state=xyz").await;
        assert!(
            page.contains("ReMa is connected to Jobs &lt;MCP&gt;"),
            "{page}"
        );
        assert_eq!(
            waiting.await.unwrap().unwrap(),
            "/callback?code=abc&state=xyz"
        );
    }
}
