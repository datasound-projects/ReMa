//! Remote MCP servers that require sign-in, against a mock server that
//! speaks the MCP authorization flow: the 401 challenge, protected
//! resource metadata, authorization server metadata, client registration
//! (Dynamic Client Registration, or a Client ID Metadata Document when the
//! server supports it), authorization code + PKCE (S256) through the
//! loopback redirect, the token exchange with the RFC 8707 `resource`,
//! refresh, expiry, rejected refreshes, a 401 during use, sign-out, and
//! what a malformed or impostor authorization server does. Tokens never
//! reach the database.

use std::sync::{Arc, Mutex};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use reqwest::Url;
use sha2::{Digest, Sha256};

use crate::{
    error::AppResult,
    llm::fake::FakeLanguageModel,
    mcp::{client, config::oauth_account},
    models::mcp::{McpAuth, McpServerInput, McpState, McpTransport},
    services::mcp as service,
    state::{testing, AppState},
    test_support::{MockServer, Recorded, Reply},
};

/// How the mock authorization server presents itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Metadata {
    Normal,
    /// Not JSON at all.
    Malformed,
    /// Claims to be another issuer than the one the resource named.
    WrongIssuer,
    /// No registration endpoint and no URL-based client IDs.
    NoRegistration,
    /// Accepts Client ID Metadata Documents.
    Cimd,
}

struct Realm {
    base: Mutex<String>,
    metadata: Mutex<Metadata>,
    /// The PKCE challenge the browser saw in the authorization URL.
    challenge: Mutex<Option<String>>,
    /// Access tokens the resource accepts.
    valid: Mutex<Vec<String>>,
    refresh_ok: Mutex<bool>,
    issued: Mutex<u32>,
}

impl Realm {
    fn base(&self) -> String {
        self.base.lock().unwrap().clone()
    }

    fn metadata_json(&self) -> String {
        let base = self.base();
        let mode = *self.metadata.lock().unwrap();
        if mode == Metadata::Malformed {
            return "this is not metadata {".into();
        }
        let issuer = if mode == Metadata::WrongIssuer {
            "http://impostor.invalid/as".to_string()
        } else {
            format!("{base}/as")
        };
        let mut json = serde_json::json!({
            "issuer": issuer,
            "authorization_endpoint": format!("{base}/as/authorize"),
            "token_endpoint": format!("{base}/as/token"),
            "response_types_supported": ["code"],
            "grant_types_supported": ["authorization_code", "refresh_token"],
            "code_challenge_methods_supported": ["S256"],
            "token_endpoint_auth_methods_supported": ["none"],
        });
        if mode != Metadata::NoRegistration {
            json["registration_endpoint"] = serde_json::json!(format!("{base}/as/register"));
        }
        if mode == Metadata::Cimd {
            json["client_id_metadata_document_supported"] = serde_json::json!(true);
        }
        json.to_string()
    }

    fn bearer(&self, req: &Recorded) -> Option<String> {
        req.headers
            .lines()
            .find_map(|l| l.strip_prefix("authorization: bearer "))
            .map(|t| t.trim().to_string())
    }

    fn challenge_for(&self, req: &Recorded) -> Reply {
        Reply::new(401, "{}")
            .header(
                "WWW-Authenticate",
                &format!(
                    "Bearer resource_metadata=\"{}/.well-known/oauth-protected-resource/mcp\"",
                    self.base()
                ),
            )
            .header("X-Request", &req.method)
    }

    fn handle(&self, req: &Recorded) -> Option<Reply> {
        let base = self.base();
        let path = req.target.split('?').next().unwrap_or("");
        match (req.method.as_str(), path) {
            ("GET", "/.well-known/oauth-protected-resource/mcp") => Some(Reply::new(
                200,
                serde_json::json!({
                    "resource": format!("{base}/mcp"),
                    "authorization_servers": [format!("{base}/as")],
                })
                .to_string(),
            )),
            ("GET", "/.well-known/oauth-authorization-server/as") => {
                Some(Reply::new(200, self.metadata_json()))
            }
            ("POST", "/as/register") => {
                let request: serde_json::Value = serde_json::from_str(&req.body).ok()?;
                Some(Reply::new(
                    201,
                    serde_json::json!({
                        "client_id": "dcr-client-1",
                        "redirect_uris": request["redirect_uris"],
                        "token_endpoint_auth_method": "none",
                        "grant_types": request["grant_types"],
                        "response_types": ["code"],
                        "client_name": request["client_name"],
                    })
                    .to_string(),
                ))
            }
            ("POST", "/as/token") => {
                let form = form(&req.body);
                match form.get("grant_type").map(String::as_str) {
                    Some("authorization_code") => {
                        let verifier = form.get("code_verifier")?;
                        let expected = self.challenge.lock().unwrap().clone()?;
                        let digest = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
                        if digest != expected
                            || form.get("code").map(String::as_str) != Some("code-1")
                        {
                            return Some(Reply::new(400, r#"{"error":"invalid_grant"}"#));
                        }
                        if form.get("resource").map(String::as_str) != Some(&format!("{base}/mcp"))
                        {
                            return Some(Reply::new(400, r#"{"error":"invalid_target"}"#));
                        }
                        let n = {
                            let mut issued = self.issued.lock().unwrap();
                            *issued += 1;
                            *issued
                        };
                        self.valid.lock().unwrap().push(format!("at-{n}"));
                        Some(Reply::new(
                            200,
                            serde_json::json!({
                                "access_token": format!("at-{n}"), "token_type": "Bearer",
                                "expires_in": 3600, "refresh_token": format!("rt-{n}"),
                            })
                            .to_string(),
                        ))
                    }
                    Some("refresh_token") => {
                        if !*self.refresh_ok.lock().unwrap() {
                            return Some(Reply::new(
                                400,
                                r#"{"error":"invalid_grant","error_description":"revoked"}"#,
                            ));
                        }
                        let n = {
                            let mut issued = self.issued.lock().unwrap();
                            *issued += 1;
                            *issued
                        };
                        self.valid.lock().unwrap().push(format!("at-{n}"));
                        Some(Reply::new(
                            200,
                            serde_json::json!({
                                "access_token": format!("at-{n}"), "token_type": "Bearer",
                                "expires_in": 3600, "refresh_token": format!("rt-{n}"),
                            })
                            .to_string(),
                        ))
                    }
                    _ => Some(Reply::new(400, r#"{"error":"unsupported_grant_type"}"#)),
                }
            }
            ("GET" | "DELETE", "/mcp") => Some(Reply::new(405, "")),
            ("POST", "/mcp") => {
                let Some(token) = self.bearer(req) else {
                    return Some(self.challenge_for(req));
                };
                if !self.valid.lock().unwrap().contains(&token) {
                    return Some(self.challenge_for(req));
                }
                let message: serde_json::Value = serde_json::from_str(&req.body).ok()?;
                let id = message["id"].clone();
                let result = match message["method"].as_str()? {
                    "initialize" => serde_json::json!({
                        "protocolVersion": message["params"]["protocolVersion"],
                        "capabilities": {"tools": {}},
                        "serverInfo": {"name": "Mock Issues", "version": "1.0"},
                    }),
                    "notifications/initialized" => return Some(Reply::new(202, "")),
                    "tools/list" => serde_json::json!({
                        "tools": [{
                            "name": "list_issues",
                            "description": "Lists issues",
                            "inputSchema": {"type": "object", "properties": {}},
                        }]
                    }),
                    "tools/call" => serde_json::json!({
                        "content": [{"type": "text", "text": "3 issues"}],
                        "isError": false,
                    }),
                    _ => return Some(Reply::new(404, "{}")),
                };
                Some(Reply::new(
                    200,
                    serde_json::json!({"jsonrpc": "2.0", "id": id, "result": result}).to_string(),
                ))
            }
            _ => None,
        }
    }
}

fn form(body: &str) -> std::collections::HashMap<String, String> {
    Url::parse(&format!("http://form.invalid/?{body}"))
        .unwrap()
        .query_pairs()
        .into_owned()
        .collect()
}

fn query(url: &str) -> std::collections::HashMap<String, String> {
    Url::parse(url)
        .unwrap()
        .query_pairs()
        .into_owned()
        .collect()
}

async fn realm() -> (Arc<Realm>, MockServer) {
    let realm = Arc::new(Realm {
        base: Mutex::new(String::new()),
        metadata: Mutex::new(Metadata::Normal),
        challenge: Mutex::default(),
        valid: Mutex::default(),
        refresh_ok: Mutex::new(true),
        issued: Mutex::new(0),
    });
    let handler = realm.clone();
    let server = MockServer::start_with_headers(move |req| handler.handle(req)).await;
    *realm.base.lock().unwrap() = server.base_url.clone();
    (realm, server)
}

/// Plays the browser: records the authorization URL, keeps the PKCE
/// challenge for the token endpoint to verify, and follows the redirect
/// back to ReMa's loopback address with a code.
#[derive(Clone, Default)]
struct Browser {
    opened: Arc<Mutex<Vec<String>>>,
    realm: Option<Arc<Realm>>,
    /// Answer with this `state` instead of the one asked for.
    forge_state: bool,
}

impl Browser {
    fn for_realm(realm: &Arc<Realm>) -> Self {
        Self {
            realm: Some(realm.clone()),
            ..Self::default()
        }
    }

    fn open(&self) -> impl Fn(&str) -> AppResult<()> + '_ {
        move |url: &str| {
            self.opened.lock().unwrap().push(url.to_string());
            let q = query(url);
            if let Some(realm) = &self.realm {
                *realm.challenge.lock().unwrap() = q.get("code_challenge").cloned();
            }
            let state = if self.forge_state {
                "forged".to_string()
            } else {
                q["state"].clone()
            };
            let target = format!("{}?code=code-1&state={state}", q["redirect_uri"]);
            tokio::spawn(async move {
                let _ = reqwest::get(target).await;
            });
            Ok(())
        }
    }

    fn opened(&self) -> usize {
        self.opened.lock().unwrap().len()
    }

    fn last(&self) -> String {
        self.opened.lock().unwrap().last().cloned().unwrap()
    }
}

fn input(url: &str, auth: McpAuth) -> McpServerInput {
    McpServerInput {
        name: "Issues".into(),
        transport: McpTransport::Http,
        command: String::new(),
        args: Vec::new(),
        env: Vec::new(),
        cwd: String::new(),
        url: url.into(),
        auth,
        header_name: String::new(),
        secret: None,
    }
}

fn state() -> AppState {
    testing::state(Arc::new(FakeLanguageModel::replying(&[]))).0
}

/// The server's stored OAuth credentials (what the keychain holds).
async fn stored(state: &AppState, id: i64) -> Option<serde_json::Value> {
    state
        .vault
        .get_text(&oauth_account(id))
        .await
        .unwrap()
        .map(|raw| serde_json::from_str(&raw).unwrap())
}

/// Lets the stored access token expire (as time would).
async fn expire(state: &AppState, id: i64) {
    let mut creds = stored(state, id).await.expect("signed in");
    creds["token_received_at"] = serde_json::json!(1);
    state
        .vault
        .set_text(&oauth_account(id), &creds.to_string())
        .await
        .unwrap();
}

fn everything_in_the_database(state: &AppState) -> String {
    state
        .db
        .call(|c| {
            let mut out = String::new();
            let mut statement = c.prepare("SELECT * FROM mcp_servers")?;
            let columns = statement.column_count();
            let mut rows = statement.query([])?;
            while let Some(row) = rows.next()? {
                for i in 0..columns {
                    let value: rusqlite::types::Value = row.get(i)?;
                    out.push_str(&format!("{value:?}\n"));
                }
            }
            Ok(out)
        })
        .unwrap()
}

#[tokio::test]
async fn a_server_added_by_url_is_discovered_signed_in_and_stays_connected() {
    let (realm, mock) = realm().await;
    let state = state();
    let url = format!("{}/mcp", mock.base_url);

    // Test before saving: the probe says what the server wants.
    let probe = service::test(&state, None, input(&url, McpAuth::None))
        .await
        .unwrap();
    assert!(!probe.ok && probe.requires_sign_in, "{probe:?}");
    assert!(
        probe.message.contains("requires sign-in"),
        "{}",
        probe.message
    );

    // Add by URL, no authentication chosen: the first connection finds the
    // 401 challenge, the row becomes an OAuth server needing sign-in.
    let server = service::save(&state, None, input(&url, McpAuth::None))
        .await
        .unwrap();
    let id = server.id;
    // Enabled in the database only: `service::set_enabled` also connects in
    // the background, which would race with the explicit probe below.
    state
        .db
        .call(|c| crate::db::mcp::set_enabled(c, id, true, crate::time::now_ms()))
        .unwrap();
    let error = service::connect_server(&state, id)
        .await
        .err()
        .expect("not connected");
    assert!(
        matches!(error, client::ConnectError::NeedsSignIn(_)),
        "{error}"
    );
    let server = service::get(&state, id).unwrap();
    assert_eq!(server.auth, McpAuth::Oauth);
    assert_eq!(server.status.state, McpState::NeedsSignIn);
    assert!(
        server
            .status
            .message
            .as_deref()
            .unwrap()
            .contains("requires sign-in"),
        "{:?}",
        server.status.message
    );
    // Nothing opened a browser on its own.
    let browser = Browser::for_realm(&realm);
    assert_eq!(browser.opened(), 0);

    // Sign in: discovery, registration, PKCE, redirect, exchange; then the
    // original request (the connection and its tools/list) is retried.
    let server = service::sign_in(&state, id, browser.open()).await.unwrap();
    assert_eq!(
        server.status.state,
        McpState::Connected,
        "{:?}",
        server.status
    );
    assert_eq!(
        server
            .status
            .tools
            .iter()
            .map(|t| t.name.as_str())
            .collect::<Vec<_>>(),
        ["list_issues"]
    );
    assert_eq!(
        server.status.server_info.as_deref(),
        Some("Mock Issues 1.0")
    );
    assert_eq!(browser.opened(), 1);

    // The authorization request: code + PKCE S256, a state, ReMa's loopback
    // redirect and the resource it is for.
    let q = query(&browser.last());
    assert!(browser
        .last()
        .starts_with(&format!("{}/as/authorize?", mock.base_url)));
    assert_eq!(q["response_type"], "code");
    assert_eq!(q["code_challenge_method"], "S256");
    assert!(q["code_challenge"].len() >= 43);
    assert!(q["state"].len() >= 16);
    assert!(q["redirect_uri"].starts_with("http://127.0.0.1:"));
    assert_eq!(q["client_id"], "dcr-client-1");
    assert_eq!(q["resource"], url);

    // Registration order: no pre-registered client, no metadata document
    // support advertised, so Dynamic Client Registration as a native
    // public client.
    let requests = mock.requests();
    let registration = requests
        .iter()
        .find(|r| r.target == "/as/register")
        .expect("registered");
    let registered: serde_json::Value = serde_json::from_str(&registration.body).unwrap();
    assert_eq!(registered["client_name"], "ReMa");
    assert_eq!(registered["token_endpoint_auth_method"], "none");
    assert_eq!(registered["application_type"], "native");
    assert!(registered.get("client_secret").is_none());
    let exchange = requests
        .iter()
        .find(|r| r.target == "/as/token")
        .expect("exchanged");
    assert!(exchange.body.contains("grant_type=authorization_code"));
    assert!(exchange.body.contains("code_verifier="));
    assert!(!exchange.body.contains("client_secret"));
    // Discovery went through the challenge's protected resource metadata.
    assert!(requests
        .iter()
        .any(|r| r.target == "/.well-known/oauth-protected-resource/mcp"));
    assert!(requests
        .iter()
        .any(|r| r.target == "/.well-known/oauth-authorization-server/as"));

    // Persisted in the credential store only.
    let creds = stored(&state, id).await.expect("stored");
    assert_eq!(creds["token_response"]["access_token"], "at-1");
    assert_eq!(creds["token_response"]["refresh_token"], "rt-1");
    let db = everything_in_the_database(&state);
    for secret in ["at-1", "rt-1", "code-1"] {
        assert!(!db.contains(secret), "{secret} in the database");
    }

    // "Restart": no connection in memory; the stored sign-in reconnects
    // without a browser.
    state.mcp.disconnect(id);
    service::connect_server(&state, id).await.unwrap();
    assert_eq!(
        service::get(&state, id).unwrap().status.state,
        McpState::Connected
    );
    assert_eq!(browser.opened(), 1);

    // An expired access token is renewed with the refresh token, silently.
    state.mcp.disconnect(id);
    expire(&state, id).await;
    let before = mock.requests().len();
    service::connect_server(&state, id).await.unwrap();
    let recent: Vec<Recorded> = mock.requests()[before..].to_vec();
    let refreshes: Vec<&Recorded> = recent.iter().filter(|r| r.target == "/as/token").collect();
    assert_eq!(refreshes.len(), 1);
    assert!(refreshes[0].body.contains("grant_type=refresh_token"));
    assert!(refreshes[0].body.contains("refresh_token=rt-1"));
    let creds = stored(&state, id).await.unwrap();
    assert_eq!(creds["token_response"]["access_token"], "at-2");
    assert_eq!(browser.opened(), 1);

    // A rejected refresh (revoked at the server) asks for a sign-in again
    // and opens nothing.
    state.mcp.disconnect(id);
    *realm.refresh_ok.lock().unwrap() = false;
    expire(&state, id).await;
    let error = service::connect_server(&state, id)
        .await
        .err()
        .expect("not connected");
    assert!(
        matches!(error, client::ConnectError::NeedsSignIn(_)),
        "{error}"
    );
    assert_eq!(
        service::get(&state, id).unwrap().status.state,
        McpState::NeedsSignIn
    );
    assert_eq!(browser.opened(), 1);

    // Sign in again: a fresh grant, and the tool works.
    *realm.refresh_ok.lock().unwrap() = true;
    let server = service::sign_in(&state, id, browser.open()).await.unwrap();
    assert_eq!(server.status.state, McpState::Connected);
    assert_eq!(browser.opened(), 2);
    let connection = state.mcp.live(id).expect("live");
    let (text, is_error) = connection
        .call(
            "list_issues",
            serde_json::Map::new(),
            &tokio_util::sync::CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!((text.as_str(), is_error), ("3 issues", false));

    // A 401 during use with a working refresh token: renewed and retried
    // inside the connection, the call succeeds.
    realm.valid.lock().unwrap().clear();
    let before = mock.requests().len();
    let (text, _) = connection
        .call(
            "list_issues",
            serde_json::Map::new(),
            &tokio_util::sync::CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(text, "3 issues");
    let recent: Vec<Recorded> = mock.requests()[before..].to_vec();
    assert!(
        recent
            .iter()
            .any(|r| r.target == "/as/token" && r.body.contains("grant_type=refresh_token")),
        "the 401 was answered with a refresh"
    );
    assert_eq!(browser.opened(), 2);

    // A 401 during use that no refresh can fix: the call fails and says
    // so; ReMa marks the server as needing sign-in rather than opening a
    // browser.
    *realm.refresh_ok.lock().unwrap() = false;
    realm.valid.lock().unwrap().clear();
    let error = connection
        .call(
            "list_issues",
            serde_json::Map::new(),
            &tokio_util::sync::CancellationToken::new(),
        )
        .await
        .unwrap_err();
    assert!(client::call_needs_sign_in(&error), "{error}");
    state
        .mcp
        .mark_needs_sign_in(id, "The server no longer accepts ReMa's sign-in.");
    assert_eq!(
        service::get(&state, id).unwrap().status.state,
        McpState::NeedsSignIn
    );
    assert_eq!(browser.opened(), 2);

    // Sign out deletes the credentials.
    service::sign_out(&state, id).await.unwrap();
    assert!(stored(&state, id).await.is_none());
    let error = service::connect_server(&state, id)
        .await
        .err()
        .expect("not connected");
    assert!(matches!(error, client::ConnectError::NeedsSignIn(_)));
}

#[tokio::test]
async fn a_forged_state_is_refused() {
    let (realm, mock) = realm().await;
    let state = state();
    let url = format!("{}/mcp", mock.base_url);
    let server = service::save(&state, None, input(&url, McpAuth::Oauth))
        .await
        .unwrap();
    let browser = Browser {
        forge_state: true,
        ..Browser::for_realm(&realm)
    };
    let error = service::sign_in(&state, server.id, browser.open())
        .await
        .unwrap_err();
    assert!(error.to_string().contains("sign-in failed"), "{error}");
    assert!(stored(&state, server.id).await.is_none());
    assert!(!mock.requests().iter().any(|r| r.target == "/as/token"));
}

#[tokio::test]
async fn malformed_metadata_and_an_impostor_issuer_stop_the_sign_in() {
    let (realm, mock) = realm().await;
    let state = state();
    let url = format!("{}/mcp", mock.base_url);
    let server = service::save(&state, None, input(&url, McpAuth::Oauth))
        .await
        .unwrap();
    let browser = Browser::for_realm(&realm);

    *realm.metadata.lock().unwrap() = Metadata::Malformed;
    let error = service::sign_in(&state, server.id, browser.open())
        .await
        .unwrap_err();
    assert!(error.to_string().contains("sign-in"), "{error}");
    assert_eq!(browser.opened(), 0, "no browser without valid metadata");

    *realm.metadata.lock().unwrap() = Metadata::WrongIssuer;
    let error = service::sign_in(&state, server.id, browser.open())
        .await
        .unwrap_err();
    assert!(error.to_string().contains("issuer mismatch"), "{error}");
    assert_eq!(browser.opened(), 0);

    *realm.metadata.lock().unwrap() = Metadata::NoRegistration;
    let error = service::sign_in(&state, server.id, browser.open())
        .await
        .unwrap_err();
    assert!(
        error.to_string().contains("does not offer OAuth sign-in"),
        "{error}"
    );
    assert_eq!(browser.opened(), 0);
    assert!(stored(&state, server.id).await.is_none());
}

#[tokio::test]
async fn a_client_id_metadata_document_replaces_registration_when_supported() {
    let (realm, mock) = realm().await;
    let state = state();
    let url = format!("{}/mcp", mock.base_url);
    let server = service::save(&state, None, input(&url, McpAuth::Oauth))
        .await
        .unwrap();
    state
        .db
        .call(|c| crate::db::mcp::set_enabled(c, server.id, true, crate::time::now_ms()))
        .unwrap();
    let browser = Browser::for_realm(&realm);
    let cancel = tokio_util::sync::CancellationToken::new();
    let document = "https://rema.example/oauth/client.json";

    // The server does not advertise support: registration as before, even
    // though ReMa has a document.
    super::oauth::sign_in_as(
        "Issues",
        &url,
        service::oauth_store(&state, server.id),
        browser.open(),
        &cancel,
        Some(document),
    )
    .await
    .unwrap();
    assert_eq!(query(&browser.last())["client_id"], "dcr-client-1");
    assert_eq!(
        mock.requests()
            .iter()
            .filter(|r| r.target == "/as/register")
            .count(),
        1
    );

    // The server supports it: the document's URL is the client ID and no
    // registration request is made.
    *realm.metadata.lock().unwrap() = Metadata::Cimd;
    service::sign_out(&state, server.id).await.unwrap();
    super::oauth::sign_in_as(
        "Issues",
        &url,
        service::oauth_store(&state, server.id),
        browser.open(),
        &cancel,
        Some(document),
    )
    .await
    .unwrap();
    assert_eq!(query(&browser.last())["client_id"], document);
    assert_eq!(
        mock.requests()
            .iter()
            .filter(|r| r.target == "/as/register")
            .count(),
        1,
        "no second registration"
    );
    // ... and the sign-in connects.
    service::connect_server(&state, server.id).await.unwrap();
    assert_eq!(
        service::get(&state, server.id).unwrap().status.state,
        McpState::Connected
    );

    // Without a document (or one that is not HTTPS) registration is used.
    assert!(
        super::oauth::client_identity("http://127.0.0.1:1/callback".into(), None)
            .client_metadata_url
            .is_none()
    );
    assert!(super::oauth::client_identity(
        "http://127.0.0.1:1/callback".into(),
        Some("http://rema.example/client.json")
    )
    .client_metadata_url
    .is_none());
}
