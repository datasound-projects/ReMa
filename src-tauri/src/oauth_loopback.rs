//! The local end of a browser sign-in: a one-shot HTTP listener on the
//! loopback interface that receives the provider's redirect (RFC 8252
//! loopback redirect for native apps).

use std::time::Duration;

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::mpsc,
};
use tokio_util::sync::CancellationToken;

use crate::error::{AppError, AppResult};

/// The pages shown in the browser once the redirect arrived.
pub struct Pages<'a> {
    /// Product name for the messages, e.g. "Google" or an MCP server name.
    pub service: &'a str,
    /// Replaces "ReMa is connected to <service>" on success.
    pub success_title: Option<&'a str>,
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

/// Loopback listeners on one port: 127.0.0.1, plus [::1] when available so
/// that a `http://localhost:<port>` redirect works whichever address the
/// browser resolves `localhost` to.
pub struct Loopback {
    listeners: Vec<TcpListener>,
    port: u16,
}

impl Loopback {
    /// 127.0.0.1 on a random free port.
    pub async fn ipv4() -> AppResult<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        Ok(Self {
            listeners: vec![listener],
            port,
        })
    }

    /// 127.0.0.1 and, if the system has IPv6 loopback, [::1] on the same port.
    pub async fn dual_stack() -> AppResult<Self> {
        let mut loopback = Self::ipv4().await?;
        if let Ok(v6) = TcpListener::bind(("::1", loopback.port)).await {
            loopback.listeners.push(v6);
        }
        Ok(loopback)
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn from_listener(listener: TcpListener) -> AppResult<Self> {
        let port = listener.local_addr()?.port();
        Ok(Self {
            listeners: vec![listener],
            port,
        })
    }
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
    receive_on(
        Loopback::from_listener(listener)?,
        is_redirect,
        succeeded,
        pages,
        cancel,
        timeout,
    )
    .await
}

/// [`receive`] on every listener of a [`Loopback`].
pub async fn receive_on(
    loopback: Loopback,
    is_redirect: impl Fn(&str) -> bool,
    succeeded: impl Fn(&str) -> bool,
    pages: Pages<'_>,
    cancel: &CancellationToken,
    timeout: Duration,
) -> AppResult<String> {
    // Every listener hands its connections to one queue; the tasks end when
    // this function returns (the guard aborts them).
    let (tx, mut rx) = mpsc::channel::<TcpStream>(8);
    let tasks: Vec<_> = loopback
        .listeners
        .into_iter()
        .map(|listener| {
            let tx = tx.clone();
            tokio::spawn(async move {
                while let Ok((socket, _)) = listener.accept().await {
                    if tx.send(socket).await.is_err() {
                        break;
                    }
                }
            })
        })
        .collect();
    drop(tx);
    struct Abort(Vec<tokio::task::JoinHandle<()>>);
    impl Drop for Abort {
        fn drop(&mut self) {
            for task in &self.0 {
                task.abort();
            }
        }
    }
    let _guard = Abort(tasks);

    let deadline = tokio::time::sleep(timeout);
    tokio::pin!(deadline);
    loop {
        let mut socket = tokio::select! {
            _ = cancel.cancelled() => {
                return Err(AppError::validation(format!("{} sign-in was cancelled.", pages.service)))
            }
            _ = &mut deadline => {
                return Err(AppError::validation(format!("{} sign-in timed out. Try again.", pages.service)))
            }
            accepted = rx.recv() => match accepted {
                Some(socket) => socket,
                None => return Err(AppError::internal("the sign-in listener stopped")),
            },
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
            let title = pages
                .success_title
                .map(str::to_string)
                .unwrap_or_else(|| format!("ReMa is connected to {}", pages.service));
            (
                "200 OK",
                page(&title, "You can close this tab and return to ReMa."),
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

    async fn get(addr: (&'static str, u16), path: &str) -> String {
        let mut s = TcpStream::connect(addr).await.unwrap();
        s.write_all(format!("GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes())
            .await
            .unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).await.unwrap();
        out
    }

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
                    success_title: None,
                },
                &cancel,
                Duration::from_secs(10),
            )
            .await
        });
        assert!(get(("127.0.0.1", port), "/favicon.ico")
            .await
            .starts_with("HTTP/1.1 404"));
        let page = get(("127.0.0.1", port), "/callback?code=abc&state=xyz").await;
        assert!(
            page.contains("ReMa is connected to Jobs &lt;MCP&gt;"),
            "{page}"
        );
        assert_eq!(
            waiting.await.unwrap().unwrap(),
            "/callback?code=abc&state=xyz"
        );
    }

    #[tokio::test]
    async fn dual_stack_accepts_whichever_loopback_address_the_browser_uses() {
        let loopback = Loopback::dual_stack().await.unwrap();
        let port = loopback.port();
        let has_v6 = loopback.listeners.len() == 2;
        let cancel = CancellationToken::new();
        let waiting = tokio::spawn(async move {
            receive_on(
                loopback,
                |t| t.contains("code="),
                |_| true,
                Pages {
                    service: "Microsoft",
                    success_title: Some("ReMa connected successfully."),
                },
                &cancel,
                Duration::from_secs(10),
            )
            .await
        });
        let host = if has_v6 { "::1" } else { "127.0.0.1" };
        let page = get((host, port), "/?code=c&state=s").await;
        assert!(page.contains("ReMa connected successfully."), "{page}");
        assert!(page.contains("You can close this tab and return to ReMa."));
        assert_eq!(waiting.await.unwrap().unwrap(), "/?code=c&state=s");
    }

    #[tokio::test]
    async fn times_out_and_can_be_cancelled() {
        let cancel = CancellationToken::new();
        let timed_out = receive_on(
            Loopback::ipv4().await.unwrap(),
            |_| true,
            |_| true,
            Pages {
                service: "Google",
                success_title: None,
            },
            &cancel,
            Duration::from_millis(50),
        )
        .await
        .unwrap_err();
        assert!(timed_out.to_string().contains("timed out"));
        cancel.cancel();
        let cancelled = receive_on(
            Loopback::ipv4().await.unwrap(),
            |_| true,
            |_| true,
            Pages {
                service: "Google",
                success_title: None,
            },
            &cancel,
            Duration::from_secs(5),
        )
        .await
        .unwrap_err();
        assert!(cancelled.to_string().contains("cancelled"));
    }
}
