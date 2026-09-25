//! A tiny scriptable HTTP server for tests of real HTTP clients.

use std::sync::{Arc, Mutex};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

#[derive(Debug, Clone)]
#[allow(dead_code)] // fields are for assertions
pub struct Recorded {
    pub method: String,
    /// Path including the query string.
    pub target: String,
    pub headers: String,
    pub body: String,
}

type Handler = dyn Fn(&Recorded) -> Option<(u16, String)> + Send + Sync;

/// Serves every request with the first handler that returns a response.
pub struct MockServer {
    pub base_url: String,
    pub requests: Arc<Mutex<Vec<Recorded>>>,
}

impl MockServer {
    pub async fn start(
        handler: impl Fn(&Recorded) -> Option<(u16, String)> + Send + Sync + 'static,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let requests: Arc<Mutex<Vec<Recorded>>> = Arc::default();
        let handler: Arc<Handler> = Arc::new(handler);
        let log = requests.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let handler = handler.clone();
                let log = log.clone();
                tokio::spawn(async move {
                    let mut data = Vec::new();
                    let mut buf = [0u8; 8192];
                    let (head_end, length) = loop {
                        let n = socket.read(&mut buf).await.unwrap_or(0);
                        if n == 0 {
                            return;
                        }
                        data.extend_from_slice(&buf[..n]);
                        let text = String::from_utf8_lossy(&data).to_string();
                        if let Some(end) = text.find("\r\n\r\n") {
                            let length = text[..end]
                                .lines()
                                .find_map(|l| {
                                    l.to_ascii_lowercase()
                                        .strip_prefix("content-length:")
                                        .and_then(|v| v.trim().parse::<usize>().ok())
                                })
                                .unwrap_or(0);
                            break (end + 4, length);
                        }
                    };
                    while data.len() < head_end + length {
                        let n = socket.read(&mut buf).await.unwrap_or(0);
                        if n == 0 {
                            break;
                        }
                        data.extend_from_slice(&buf[..n]);
                    }
                    let text = String::from_utf8_lossy(&data).to_string();
                    let head = &text[..head_end];
                    let mut first = head.lines().next().unwrap_or("").split_whitespace();
                    let recorded = Recorded {
                        method: first.next().unwrap_or("").into(),
                        target: first.next().unwrap_or("").into(),
                        headers: head.to_ascii_lowercase(),
                        body: text[head_end..].into(),
                    };
                    let (status, body) = handler(&recorded).unwrap_or((404, "{}".into()));
                    log.lock().unwrap().push(recorded);
                    let response = format!(
                        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                });
            }
        });
        Self { base_url, requests }
    }

    pub fn requests(&self) -> Vec<Recorded> {
        self.requests.lock().unwrap().clone()
    }
}
