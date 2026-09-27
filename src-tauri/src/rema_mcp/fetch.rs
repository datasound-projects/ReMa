//! Network access for ReMa MCP (OWASP SSRF guidance):
//!
//! - GET only, http(s) on the standard ports, no URL credentials, no
//!   cookies, no authorization headers, certificate checks always on.
//! - Public addresses only, enforced where the connection is made: a DNS
//!   resolver that drops loopback, private, link-local, CGNAT, multicast
//!   and metadata addresses (so a name cannot be re-pointed after a check),
//!   plus the same check for IP literals. Every redirect is checked again.
//! - Bounded redirects, time, bytes (no compression is negotiated, so the
//!   limit is on what is parsed), concurrency (6 in total, 2 per host).
//! - `robots.txt` is honored for pages; documented APIs are used as their
//!   documentation describes.
//! - One retry for transient failures (connection errors, 429, 503), with
//!   jitter and `Retry-After`, never beyond the request's deadline.

use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use futures_util::StreamExt;
use reqwest::{header, redirect, StatusCode, Url};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

use super::sources::check_url_with;
use crate::analytics::page::is_public;

pub const MAX_REDIRECTS: usize = 5;
pub const PAGE_LIMIT: usize = 2 * 1024 * 1024;
pub const FEED_LIMIT: usize = 8 * 1024 * 1024;
const ROBOTS_LIMIT: usize = 256 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const GLOBAL_CONCURRENCY: usize = 6;
const PER_HOST: usize = 2;
const ROBOTS_TTL: Duration = Duration::from_secs(3600);
/// The product token matched against robots.txt groups.
pub const AGENT_TOKEN: &str = "ReMa";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchError {
    /// The source's policy (robots.txt) or ReMa's network policy forbids it.
    Blocked(String),
    /// 404 / 410.
    Gone(u16),
    /// 401 / 403 / 451 / 999: the site refuses automated reading.
    Refused(u16),
    RateLimited,
    Timeout,
    TooLarge,
    Cancelled,
    Failed(String),
}

impl FetchError {
    pub fn message(&self) -> String {
        match self {
            Self::Blocked(why) => why.clone(),
            Self::Gone(code) => format!("the source no longer serves this page ({code})"),
            Self::Refused(code) => format!("the site refuses automated reading ({code})"),
            Self::RateLimited => "the source is rate limiting requests".into(),
            Self::Timeout => "the source did not answer in time".into(),
            Self::TooLarge => "the response is larger than ReMa reads".into(),
            Self::Cancelled => "stopped".into(),
            Self::Failed(why) => why.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Accept {
    Html,
    Json,
    Xml,
    Text,
}

impl Accept {
    fn header(self) -> &'static str {
        match self {
            Self::Html => "text/html,application/xhtml+xml",
            Self::Json => "application/json",
            Self::Xml => "application/xml,text/xml",
            Self::Text => "text/plain",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Validators {
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Response {
    pub final_url: String,
    /// `None` for 304 Not Modified.
    pub body: Option<String>,
    pub truncated: bool,
    pub validators: Validators,
}

/// Resolves host names to public addresses only.
struct PublicResolver;

impl reqwest::dns::Resolve for PublicResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let host = name.as_str().to_string();
        Box::pin(async move {
            let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), 0))
                .await?
                .filter(|a| is_public(a.ip()))
                .collect();
            if addrs.is_empty() {
                return Err("the name does not resolve to a public address".into());
            }
            Ok(Box::new(addrs.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

/// (user agents, [(allow, path pattern)])
type Group = (Vec<String>, Vec<(bool, String)>);

#[derive(Clone)]
struct Robots {
    groups: Vec<Group>,
}

impl Robots {
    fn parse(text: &str) -> Self {
        let mut groups: Vec<Group> = Vec::new();
        let mut agents_open = false;
        for line in text.lines() {
            let line = line.split('#').next().unwrap_or_default().trim();
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let (key, value) = (key.trim().to_lowercase(), value.trim().to_string());
            match key.as_str() {
                "user-agent" => {
                    if !agents_open {
                        groups.push((Vec::new(), Vec::new()));
                        agents_open = true;
                    }
                    if let Some(g) = groups.last_mut() {
                        g.0.push(value.to_lowercase());
                    }
                }
                "allow" | "disallow" => {
                    agents_open = false;
                    if let Some(g) = groups.last_mut() {
                        g.1.push((key == "allow", value));
                    }
                }
                _ => {}
            }
        }
        Self { groups }
    }

    fn rule_matches(pattern: &str, path: &str) -> Option<usize> {
        if pattern.is_empty() {
            return None;
        }
        let anchored = pattern.ends_with('$');
        let pattern = pattern.trim_end_matches('$');
        let parts: Vec<&str> = pattern.split('*').collect();
        let mut pos = 0;
        for (i, part) in parts.iter().enumerate() {
            if i == 0 {
                if !path.starts_with(part) {
                    return None;
                }
                pos = part.len();
            } else {
                pos = pos + path[pos..].find(part)? + part.len();
            }
        }
        if anchored && pos != path.len() && !pattern.ends_with('*') {
            return None;
        }
        Some(pattern.len())
    }

    /// Longest matching rule wins; Allow wins a tie.
    fn allowed(&self, path: &str) -> bool {
        let token = AGENT_TOKEN.to_lowercase();
        let group = self
            .groups
            .iter()
            .find(|(agents, _)| {
                agents
                    .iter()
                    .any(|a| a != "*" && token.contains(a.as_str()))
            })
            .or_else(|| {
                self.groups
                    .iter()
                    .find(|(agents, _)| agents.iter().any(|a| a == "*"))
            });
        let Some((_, rules)) = group else {
            return true;
        };
        let mut best: Option<(usize, bool)> = None;
        for (allow, pattern) in rules {
            if let Some(len) = Self::rule_matches(pattern, path) {
                if best.is_none_or(|(l, a)| len > l || (len == l && *allow && !a)) {
                    best = Some((len, *allow));
                }
            }
        }
        best.is_none_or(|(_, allow)| allow)
    }
}

/// ReMa MCP's HTTP client. Cheap to clone.
#[derive(Clone)]
pub struct Fetcher {
    client: reqwest::Client,
    global: Arc<Semaphore>,
    hosts: Arc<Mutex<HashMap<String, Arc<Semaphore>>>>,
    robots: Arc<Mutex<HashMap<String, (Instant, Robots)>>>,
    allow_private: bool,
}

impl Fetcher {
    /// `allow_private` is for tests and debug builds with local fixtures.
    pub fn new(version: &str, allow_private: bool) -> Self {
        let mut builder = reqwest::Client::builder()
            .redirect(redirect::Policy::none())
            .connect_timeout(Duration::from_secs(8))
            .user_agent(format!(
                "Mozilla/5.0 (compatible; {AGENT_TOKEN}/{version}; job search on behalf of a user)"
            ));
        if !allow_private {
            builder = builder.dns_resolver(PublicResolver);
        }
        Self {
            client: builder.build().unwrap_or_default(),
            global: Arc::new(Semaphore::new(GLOBAL_CONCURRENCY)),
            hosts: Arc::default(),
            robots: Arc::default(),
            allow_private,
        }
    }

    fn check(&self, url: &Url) -> Result<(), FetchError> {
        check_url_with(url.as_str(), self.allow_private).map_err(FetchError::Blocked)?;
        if self.allow_private {
            return Ok(());
        }
        let host = url.host_str().unwrap_or_default().to_lowercase();
        let local =
            || FetchError::Blocked("the address is on this computer or the local network".into());
        match host.trim_matches(['[', ']']).parse::<std::net::IpAddr>() {
            Ok(ip) if !is_public(ip) => return Err(local()),
            Ok(_) => {}
            Err(_) => {
                if host == "localhost" || host.ends_with(".localhost") || host.ends_with(".local") {
                    return Err(local());
                }
            }
        }
        Ok(())
    }

    fn host_permit(&self, host: &str) -> Arc<Semaphore> {
        self.hosts
            .lock()
            .unwrap()
            .entry(host.to_string())
            .or_insert_with(|| Arc::new(Semaphore::new(PER_HOST)))
            .clone()
    }

    /// One GET with redirects, limits, one transient retry and the deadline.
    pub async fn get(
        &self,
        url: &str,
        accept: Accept,
        limit: usize,
        validators: Option<&Validators>,
        deadline: Instant,
        cancel: &CancellationToken,
    ) -> Result<Response, FetchError> {
        let mut attempt = 0;
        loop {
            match self
                .get_once(url, accept, limit, validators, deadline, cancel)
                .await
            {
                Err((error, wait)) if attempt == 0 && wait.is_some() => {
                    let wait = wait.unwrap_or_default();
                    let jitter = Duration::from_millis(u64::from(rand_u16() % 250));
                    let wait = wait + jitter;
                    if Instant::now() + wait >= deadline {
                        return Err(error);
                    }
                    tokio::select! {
                        _ = cancel.cancelled() => return Err(FetchError::Cancelled),
                        _ = tokio::time::sleep(wait) => {}
                    }
                    attempt += 1;
                }
                Err((error, _)) => return Err(error),
                Ok(response) => return Ok(response),
            }
        }
    }

    /// An error, and how long to wait before one retry when it is transient.
    async fn get_once(
        &self,
        url: &str,
        accept: Accept,
        limit: usize,
        validators: Option<&Validators>,
        deadline: Instant,
        cancel: &CancellationToken,
    ) -> Result<Response, (FetchError, Option<Duration>)> {
        let fail = |e: FetchError| (e, None);
        let mut current =
            Url::parse(url).map_err(|_| fail(FetchError::Blocked("not a web address".into())))?;
        for _ in 0..=MAX_REDIRECTS {
            self.check(&current).map_err(fail)?;
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(fail(FetchError::Timeout));
            }
            let host = current.host_str().unwrap_or_default().to_lowercase();
            let host_permit = self.host_permit(&host);
            let _host = tokio::select! {
                _ = cancel.cancelled() => return Err(fail(FetchError::Cancelled)),
                permit = host_permit.acquire_owned() => permit.map_err(|_| fail(FetchError::Cancelled))?,
            };
            let _global = tokio::select! {
                _ = cancel.cancelled() => return Err(fail(FetchError::Cancelled)),
                permit = self.global.clone().acquire_owned() => permit.map_err(|_| fail(FetchError::Cancelled))?,
            };
            let mut request = self
                .client
                .get(current.clone())
                .timeout(remaining.min(REQUEST_TIMEOUT))
                .header(header::ACCEPT, accept.header());
            if let Some(v) = validators {
                if let Some(etag) = &v.etag {
                    request = request.header(header::IF_NONE_MATCH, etag);
                }
                if let Some(modified) = &v.last_modified {
                    request = request.header(header::IF_MODIFIED_SINCE, modified);
                }
            }
            let response = tokio::select! {
                _ = cancel.cancelled() => return Err(fail(FetchError::Cancelled)),
                response = request.send() => response,
            };
            let response = match response {
                Ok(r) => r,
                Err(e) if e.is_timeout() => return Err((FetchError::Timeout, None)),
                Err(e) if e.is_connect() => {
                    return Err((
                        FetchError::Failed("could not connect to the source".into()),
                        Some(Duration::from_millis(500)),
                    ))
                }
                Err(_) => return Err(fail(FetchError::Failed("the request failed".into()))),
            };
            let status = response.status();
            if status.is_redirection() && status != StatusCode::NOT_MODIFIED {
                let location = response
                    .headers()
                    .get(header::LOCATION)
                    .and_then(|v| v.to_str().ok())
                    .ok_or_else(|| {
                        fail(FetchError::Failed("a redirect without a target".into()))
                    })?;
                current = current
                    .join(location)
                    .map_err(|_| fail(FetchError::Failed("an invalid redirect".into())))?;
                continue;
            }
            let validators = Validators {
                etag: header_text(&response, header::ETAG),
                last_modified: header_text(&response, header::LAST_MODIFIED),
            };
            if status == StatusCode::NOT_MODIFIED {
                return Ok(Response {
                    final_url: current.to_string(),
                    body: None,
                    truncated: false,
                    validators,
                });
            }
            let retry_after = response
                .headers()
                .get(header::RETRY_AFTER)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.trim().parse::<u64>().ok())
                .map(Duration::from_secs);
            match status.as_u16() {
                404 | 410 => return Err(fail(FetchError::Gone(status.as_u16()))),
                401 | 403 | 451 | 999 => return Err(fail(FetchError::Refused(status.as_u16()))),
                429 => {
                    return Err((
                        FetchError::RateLimited,
                        Some(retry_after.unwrap_or(Duration::from_secs(1))),
                    ))
                }
                503 => {
                    return Err((
                        FetchError::Failed("the source is temporarily unavailable (503)".into()),
                        Some(retry_after.unwrap_or(Duration::from_secs(1))),
                    ))
                }
                s if !(200..300).contains(&s) => {
                    return Err(fail(FetchError::Failed(format!("the source answered {s}"))))
                }
                _ => {}
            }
            let mut body = Vec::new();
            let mut truncated = false;
            let mut stream = response.bytes_stream();
            loop {
                let chunk = tokio::select! {
                    _ = cancel.cancelled() => return Err(fail(FetchError::Cancelled)),
                    _ = tokio::time::sleep_until(deadline.into()) => return Err(fail(FetchError::Timeout)),
                    chunk = stream.next() => chunk,
                };
                let Some(chunk) = chunk else { break };
                let chunk =
                    chunk.map_err(|_| fail(FetchError::Failed("the download failed".into())))?;
                body.extend_from_slice(&chunk);
                if body.len() > limit {
                    if accept == Accept::Html {
                        body.truncate(limit);
                        truncated = true;
                        break;
                    }
                    return Err(fail(FetchError::TooLarge));
                }
            }
            return Ok(Response {
                final_url: current.to_string(),
                body: Some(String::from_utf8_lossy(&body).into_owned()),
                truncated,
                validators,
            });
        }
        Err(fail(FetchError::Failed("too many redirects".into())))
    }

    /// Whether robots.txt lets ReMa read this page. Missing (4xx) robots
    /// files allow everything; an unreadable one (5xx, offline) is an error.
    pub async fn robots_allow(
        &self,
        url: &str,
        deadline: Instant,
        cancel: &CancellationToken,
    ) -> Result<bool, FetchError> {
        let parsed =
            Url::parse(url).map_err(|_| FetchError::Blocked("not a web address".into()))?;
        let origin = parsed.origin().ascii_serialization();
        let path = match parsed.query() {
            Some(q) => format!("{}?{q}", parsed.path()),
            None => parsed.path().to_string(),
        };
        let cached = self
            .robots
            .lock()
            .unwrap()
            .get(&origin)
            .filter(|(at, _)| at.elapsed() < ROBOTS_TTL)
            .map(|(_, r)| r.clone());
        let robots = match cached {
            Some(robots) => robots,
            None => {
                let robots = match self
                    .get(
                        &format!("{origin}/robots.txt"),
                        Accept::Text,
                        ROBOTS_LIMIT,
                        None,
                        deadline,
                        cancel,
                    )
                    .await
                {
                    Ok(response) => Robots::parse(response.body.as_deref().unwrap_or_default()),
                    Err(FetchError::Gone(_) | FetchError::Refused(_)) => Robots { groups: vec![] },
                    Err(FetchError::Failed(why)) if why.contains("answered 4") => {
                        Robots { groups: vec![] }
                    }
                    Err(e) => return Err(e),
                };
                self.robots
                    .lock()
                    .unwrap()
                    .insert(origin, (Instant::now(), robots.clone()));
                robots
            }
        };
        Ok(robots.allowed(&path))
    }
}

fn header_text(response: &reqwest::Response, name: header::HeaderName) -> Option<String> {
    response
        .headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
}

/// A little randomness for retry jitter (no extra dependency).
fn rand_u16() -> u16 {
    let mut bytes = [0u8; 2];
    let _ = getrandom::fill(&mut bytes);
    u16::from_le_bytes(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::MockServer;

    fn later() -> Instant {
        Instant::now() + Duration::from_secs(10)
    }

    #[test]
    fn follows_robots_rules() {
        let robots = Robots::parse(
            "User-agent: *\nDisallow: /private\nAllow: /private/jobs\nDisallow: /*.pdf$\n\n\
             User-agent: BadBot\nDisallow: /",
        );
        assert!(robots.allowed("/jobs/1"));
        assert!(!robots.allowed("/private/x"));
        assert!(robots.allowed("/private/jobs/1"));
        assert!(!robots.allowed("/file.pdf"));
        let ours = Robots::parse("User-agent: ReMa\nDisallow: /\n\nUser-agent: *\nAllow: /");
        assert!(!ours.allowed("/jobs/1"));
        assert!(Robots::parse("").allowed("/anything"));
        assert!(Robots::parse("User-agent: *\nDisallow:").allowed("/x"));
    }

    #[tokio::test]
    async fn never_reaches_the_local_network() {
        let server = MockServer::start(|_| Some((200, "<html>secret</html>".into()))).await;
        let fetcher = Fetcher::new("test", false);
        let cancel = CancellationToken::new();
        for url in [
            server.base_url.clone(),
            "http://127.0.0.1/".to_string(),
            "http://[::1]/".into(),
            "http://[::ffff:127.0.0.1]/".into(),
            "http://0x7f.0.0.1/".into(),
            "http://2130706433/".into(),
            "http://169.254.169.254/latest/meta-data/".into(),
            "http://10.0.0.8/".into(),
            "http://localhost/".into(),
            "http://metadata.local/".into(),
        ] {
            let result = fetcher
                .get(&url, Accept::Html, PAGE_LIMIT, None, later(), &cancel)
                .await;
            assert!(
                matches!(
                    result,
                    Err(FetchError::Blocked(_)) | Err(FetchError::Failed(_))
                ),
                "{url}: {result:?}"
            );
        }
        assert!(
            server.requests().is_empty(),
            "no request reached the server"
        );
    }

    #[tokio::test]
    async fn names_that_resolve_to_private_addresses_are_refused_at_connect_time() {
        // "localhost" style names are refused by name; a name that resolves
        // to 127.0.0.1 through DNS is refused by the resolver.
        let resolver = PublicResolver;
        let name: reqwest::dns::Name = "localhost".parse().unwrap();
        let result = reqwest::dns::Resolve::resolve(&resolver, name).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn bounds_redirects_size_and_retries_once() {
        let hits = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = hits.clone();
        let server = MockServer::start(move |r| match r.target.as_str() {
            "/loop" => Some((302, "/loop".into())),
            "/big" => Some((200, "x".repeat(PAGE_LIMIT + 10))),
            "/busy" => {
                if count.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                    Some((503, String::new()))
                } else {
                    Some((200, "{\"ok\":true}".into()))
                }
            }
            "/gone" => Some((410, String::new())),
            "/forbidden" => Some((403, String::new())),
            _ => None,
        })
        .await;
        let fetcher = Fetcher::new("test", true);
        let cancel = CancellationToken::new();
        let get = |path: &str, accept: Accept| {
            let (fetcher, cancel) = (fetcher.clone(), cancel.clone());
            let url = format!("{}{path}", server.base_url);
            async move {
                fetcher
                    .get(&url, accept, PAGE_LIMIT, None, later(), &cancel)
                    .await
            }
        };
        assert_eq!(
            get("/loop", Accept::Html).await.unwrap_err(),
            FetchError::Failed("too many redirects".into())
        );
        assert_eq!(
            server
                .requests()
                .iter()
                .filter(|r| r.target == "/loop")
                .count(),
            MAX_REDIRECTS + 1
        );
        assert_eq!(
            get("/big", Accept::Json).await.unwrap_err(),
            FetchError::TooLarge
        );
        let page = get("/big", Accept::Html).await.unwrap();
        assert!(page.truncated && page.body.unwrap().len() == PAGE_LIMIT);
        let busy = get("/busy", Accept::Json).await.unwrap();
        assert_eq!(busy.body.as_deref(), Some("{\"ok\":true}"));
        assert_eq!(
            hits.load(std::sync::atomic::Ordering::SeqCst),
            2,
            "one retry"
        );
        assert_eq!(
            get("/gone", Accept::Html).await.unwrap_err(),
            FetchError::Gone(410)
        );
        assert_eq!(
            get("/forbidden", Accept::Html).await.unwrap_err(),
            FetchError::Refused(403)
        );

        let stopped = CancellationToken::new();
        stopped.cancel();
        let url = format!("{}/gone", server.base_url);
        assert_eq!(
            fetcher
                .get(&url, Accept::Html, PAGE_LIMIT, None, later(), &stopped)
                .await
                .unwrap_err(),
            FetchError::Cancelled
        );
    }
}
