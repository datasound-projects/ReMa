# Native connectors — implementation record

Gmail, Google Calendar, Outlook Mail and Outlook Calendar as native
connectors: the architecture, the decisions taken, and the requirement
checklist. Registration: [registration.md](registration.md). Compliance and
data handling: [compliance.md](compliance.md). Validation:
[validation.md](validation.md).

## 1. Architecture

```text
Settings → Connectors (React)      src/components/settings/ConnectorsSection.tsx
Applications page, bell            src/pages/ApplicationsPage.tsx, src/components/layout/NotificationBell.tsx
Calendar panel                     src/components/layout/CalendarPanel.tsx
        │  typed IPC (tauri-specta)
commands/connectors.rs, commands/applications.rs   thin adapters
        │
connectors/            registry, state, connect / disconnect / require / ready     (mod.rs)
  oauth.rs             PKCE S256, state, loopback redirect, token requests
  tokens.rs            token manager: get_valid_access_token, refresh_access_token,
                       revoke_connection, requires_reauthentication
  api.rs               authenticated API client (401 → refresh + retry, 429/503 → bounded retry,
                       token only sent to the API origin)
  google/              scopes, Gmail (history.list sync), Google Calendar (events, freeBusy)
  microsoft/           scopes, Outlook mail (Graph delta), Outlook calendar (calendarView, getSchedule)
  mail.rs, calendar.rs provider-independent MailProvider / CalendarProvider, MailMessage, conflicts
  sync.rs              clients for connected accounts, one sync per connector (SyncGuard)
  legacy.rs            moves the old Google Workspace settings over (once)
        │
jobs/                  the pipeline (provider-independent)
  prefilter.rs         deterministic signals → strong / candidate / filtered
  extract.rs           headers-only triage, 13-category classifier (strict JSON, validated)
  interviews.rs        quotes checked against the email; proposed slots validated
  applications.rs      matching, status, audit timeline, interviews, fingerprints
  calendar_sync.rs     conflicts in every connected calendar, add when free, dedupe, reschedule,
                       cancel, decline
  tracker.rs           overview, detail, user and assistant changes
        │
services/mail_monitor.rs   the run of the built-in "Job Mail & Interview Sync" (the only reader of mail)
services/calendar_view.rs  the in-app calendar: provider events + ReMa interviews
services/connector_tools.rs  the assistant's mail, calendar and tracker tools
services/notifications.rs    stored notifications + OS notifications
services/background.rs       "Run ReMa in background" (tray), "Start ReMa at login"
db/migrations/0008_connectors.sql, 0009_job_mail_sync.sql
```

Gmail and Google Calendar are two cards sharing one Google account (one
token); Outlook Mail and Outlook Calendar share one Microsoft account.

## 2. Flows

**Connect** (`connectors::connect`): the union of the provider's enabled
connectors plus the new one determines the scopes → PKCE pair and 24-byte
random `state` → loopback listener (Google `127.0.0.1`, Microsoft
`localhost` on IPv4 and IPv6) → the system browser opens the provider's page
→ the redirect's `state` is compared in constant time → code exchange with
the verifier → granted scopes read from the token response → the refresh
token to the credential store, account and connectors to SQLite → one small
check per connector (§5.5). No mail is read: mail is read only by the
built-in task "Job Mail & Interview Sync". Only then does the browser tab
show "ReMa connected successfully. You can close this browser tab and return
to ReMa." (or "ReMa could not complete authorization. Return to ReMa for
details."). Sign-ins time out after 5 minutes and can be cancelled.

**Sync** (`mail_monitor::run_task` → `jobs::run`, only from Job Mail &
Interview Sync): a reading plan per mailbox (first run: the last N days of
the task's lookback; later runs: changes since the cursor, Gmail
`historyId` / Graph `deltaLink`; an expired cursor resumes from the last
success, never before the lookback; a longer lookback reads the added days
once) → dedupe by (provider, message id) → prefilter → headers-only triage
for candidates → full read and classification of relevant mail (≤ 100 per
run, oldest first across mailboxes; the same email in both mailboxes once)
→ tracker → calendar step with the connected calendars → notifications. The
cursor and the range read so far are stored in the same transaction as the
records they cover. Details: [../applications/implementation.md](../applications/implementation.md).

**Calendar step** (`calendar_sync::sync_interview`), for each confirmed,
upcoming interview: existing event? (stored id → private
`remaInterviewId` property → fingerprint of company, role, start and
conversation) → conflicts in every connected calendar → create (confirmed,
confidence ≥ 0.8, free), report a conflict (flagged and notified, nothing
created), update (reschedule to a free time), keep but mark cancelled, or
respect a deletion or a "Don't add".

**Disconnect**: stops the connector's sync and removes its mail cursor; the
account's last connector revokes the Google grant (Microsoft: deletes tokens;
account.microsoft.com is linked) and deletes the account row. Applications
and their history stay.

**Reauthentication**: `invalid_grant`, `interaction_required`,
`consent_required` or `login_required` on refresh → tokens deleted, account
`reauth_required`, one notification per day, cards show Reconnect. ReMa
never opens a sign-in by itself.

## 3. Decisions

- **One provider account per provider.** Connecting another account replaces the old one and clears its sync cursors.
- **Union of scopes, checked.** Google installed apps get no incremental authorization; a sign-in asks for all enabled connectors' scopes and ReMa reads the granted list (unticked permissions show as "Permission missing").
- **The built-in task is the automation boundary** (replaces "background sync by default"). Mail is read only by "Job Mail & Interview Sync" and only while it is on: connecting a mailbox reads nothing and records no sync, there is no per-connector background sync, "Sync now" in a connector's details (shown only while the task is on) runs that same task, and no other task can read mail. Its schedule is the scheduler's (default every hour). The tray and start-at-login options are off until the user enables them; nothing runs when ReMa is closed.
- **Lookback, not "recent mail".** The first run reads the last N days (default 30); later runs read only changes; a longer lookback reads the added days once; a shorter one reads nothing old and deletes nothing.
- **No ask mode, no preparation buffer.** Confirmed interviews (confidence ≥ 0.8) at times that are free in every connected calendar are added; a conflict is flagged and notified and nothing is created. Adding one by hand still checks conflicts first.
- **Chat and private data.** An answer to a message about mail, calendar or applications gets connector tools and no web access (hosted web search can open arbitrary pages; this closes that exfiltration path for every provider, including runtimes that run their own tool loop). Once connector data was read, every MCP call needs approval, once per call.
- **Classification without a model**: mail is still synced and prefiltered; classification waits and the run reports why.
- **Legacy Google settings**: tokens from ReMa's built-in client move to the connector entry; tokens from a user-entered client are revoked and deleted (ReMa's client cannot refresh them) and the user is asked to reconnect. The user-entered client ID and secret are deleted.
- **Single instance**: a second launch shows the running ReMa, so two processes never sync the same mailboxes.

## 4. Requirement checklist

| Requirement | Status | Where |
|---|---|---|
| Four cards (Gmail, Google Calendar, Outlook Mail, Outlook Calendar) with publisher and description | done | `models/connectors.rs`, `ConnectorsSection.tsx` |
| Official product icons | done | `ConnectorIcons.tsx` |
| `+` / spinner / green check / reauth / permission missing / error with retry and details / unavailable | done | `connectors::status_of`, `ConnectorCard` |
| Detail panel: account, status, permissions, last sync, whether Job Mail & Interview Sync is on, Reconnect, Disconnect with confirmation | done | `ConnectorDetail` |
| No client ID/secret UI; ReMa-owned registrations | done | `Apps::from_build`, old `GoogleSection` removed |
| Authorization code + PKCE S256 + random state + loopback + system browser | done | `connectors/oauth.rs`, `oauth_loopback.rs` |
| Google scopes (openid email profile, gmail.readonly, calendar.events, calendar.freebusy), union, granted check | done | `connectors/google/mod.rs` |
| Microsoft public client, `openid profile email offline_access User.Read Mail.Read Calendars.ReadWrite`, no secret | done | `connectors/microsoft/mod.rs` |
| Token manager operations; keychain only; refresh; rotation; reauth; revoke | done | `connectors/tokens.rs` |
| Gmail initial sync + `history.list`, controlled resync | done | `connectors/google/gmail.rs` |
| Graph delta with `deltaLink`, recovery on invalid delta | done | `connectors/microsoft/mail.rs` |
| Deterministic prefilter, metadata first, bodies for likely job mail | done | `jobs/prefilter.rs`, `jobs/mod.rs` |
| Normalized `MailMessage` | done | `connectors/mail.rs` |
| 13 categories + confidence, validated structured output | done | `jobs/extract.rs` |
| Tracker audit ("Application changed to Interview / Source: Gmail message / Confidence: 98%") | done | `jobs/applications.rs`, `ApplicationsPage.tsx` |
| Conflict check in every connected calendar before adding; conflicts flagged and notified | done | `jobs/calendar_sync.rs` |
| Dedupe (provider, message, application, fingerprint, event id); reschedule updates the event | done | `db/jobs.rs`, `calendar_sync.rs` |
| AI tools mail/calendar/applications, validated in Rust; approvals for changes | done | `services/connector_tools.rs` |
| Prompt-injection guards (no tools in pipeline, fenced data, schema validation, no web next to private data, MCP approval) | done | `jobs/extract.rs`, `connector_tools.rs`, `chat_tools.rs`, `chat.rs` |
| Built-in "Job Mail & Interview Sync", the only reader of mail | done | `services/tasks.rs`, `services/scheduler.rs`, `mail_monitor::run_task` |
| Notifications (in app + OS) | done | `services/notifications.rs`, `NotificationBell.tsx` |
| Explicit "Run ReMa in background" and "Start ReMa at login"; no hidden service | done | `services/background.rs`, `lib.rs` (tray, close handler, autostart) |
| Interval through the scheduler; single flight per connector | done | `services/scheduler.rs`, `connectors/sync.rs` |
| Disconnect: stop sync, delete tokens, revoke at Google, keep history | done | `connectors::disconnect` |
| Compliance documentation | done | [compliance.md](compliance.md) |
| Tests and acceptance run | done | [validation.md](validation.md) |
| Live sign-in against real Google/Microsoft | **not done here** | needs the publisher's registrations; see validation.md |

## 5. Production connectors (specification B)

Specification: `05d000ff-ReMa_Production_Desktop_Connectors_Google_Microsoft.md`
(§1–§93). Root cause and gap analysis:
[../production-plan.md §6–§8](../production-plan.md). Validation:
[validation.md §4](validation.md).

### 5.1 Why every build said "sign-in is not available"

ReMa's Google and Microsoft client IDs were read with `option_env!` at
compile time and nothing supplied them: no file in the repository, no CI
variable, no release workflow, no check. Every build — release included —
compiled them to `None`, and the cards showed "Unavailable". The provider
code itself was complete.

### 5.2 Build configuration (B §5–§7, §80)

```text
src-tauri/connectors.toml            ReMa's public client configuration (committed)
  + GOOGLE_DESKTOP_CLIENT_ID, GOOGLE_DESKTOP_CLIENT_SECRET,
    MICROSOFT_PUBLIC_CLIENT_ID, MICROSOFT_TENANT, LINKEDIN_* (override)
        │  build.rs → connectors/build_config.rs (validate)
        ├─ release profile, Google or Microsoft missing/malformed → build fails, naming each setting
        ├─ development build, missing → one Cargo warning; cards say how to add them
        ▼
$OUT_DIR/connector_apps.rs → connectors::config (constants) → Apps::from_build()
```

- Client IDs are identifiers, not secrets (desktop apps are public
  clients); nothing obfuscates them. Release builds must use a multi-tenant
  Microsoft authority (`common`, `organizations`, `consumers`).
- **Deviation from B §20, with evidence:** Google's token endpoint requires
  the "Desktop app" client's secret (`invalid_request: client_secret is
  missing` without it, with PKCE too — reported by several projects in
  2026), while Google treats it as non-confidential for installed apps.
  ReMa therefore requires it next to the client ID and ships it as public
  configuration; security rests on PKCE, `state` and the loopback redirect.
  Microsoft gets no secret (the token request fails if one is sent).
- `.github/workflows/release.yml` packages Linux, macOS and Windows with the
  registrations from Actions variables (the Google secret from an Actions
  secret, so logs mask it); a missing value fails the job in `build.rs`.
- `release_checks.rs` (every `cargo test`): the Tauri npm packages match the
  Rust crate (packaging found `tauri 2.12.0` against `@tauri-apps/api
  2.11.1`, which makes `tauri build` refuse to package; fixed), the opener
  plugin is registered, and no capability grants a webview `opener:` or
  `shell:` permissions.
- Start-up log: `[connector] config google=ready microsoft=ready linkedin=off tenant=common source=…`.

### 5.3 Sign-in (B §9–§14, §52–§55)

`connectors::connect` → `sign_in`:

1. One sign-in per provider (`begin_sign_in` refuses a second with a
   Conflict error; Cancel ends the first). The card says "Opening Google
   sign-in…" until the browser opened, then "Finish signing in with Google
   in your browser."
2. PKCE (48 random bytes → 64-character verifier, S256), 24-byte `state`.
3. The loopback listener is bound **before** the URL is built: Google and
   LinkedIn `http://127.0.0.1:<random port>`, Microsoft
   `http://localhost:<random port>` on IPv4 and IPv6 (the port of a
   `localhost` redirect is not matched by Microsoft).
4. The backend opens the provider's page in the default browser through the
   opener plugin; the frontend only calls `connect_connector(id)`.
5. `oauth::wait_for_callback`: only `/` with `code` or `error` is the
   redirect (other paths get 404); `state` is compared in constant time;
   the first redirect closes every listener, so repeated or late callbacks
   find nothing; 5-minute timeout; cancel.
6. Code exchange (`TokenPhase::Exchange`), identity (ID token; Microsoft also
   Graph `/me`), grant stored, account and connectors saved, checks (§5.5).
7. The browser tab, held open until then, shows the success or failure
   page. Nothing from the redirect is echoed.

### 5.4 Token manager (B §27, §38–§40, §57–§58, §69)

| Aspect | Behaviour |
|---|---|
| Key | `connector:<provider>:<provider account id>` (Google `sub`, Microsoft `oid`), never the email. A grant under the old `connector:<provider>` key moves on first use; a still-valid access token it held is used until it expires. |
| Stored | Google and Microsoft: the refresh token only (`Credential::RefreshToken`). LinkedIn (no refresh tokens): its 60-day access token. |
| Memory | Access tokens with their expiry, renewed 5 minutes early; gone on restart (the first request refreshes). |
| Single flight | One refresh lock per account; callers that waited reuse the new token; a forced refresh after a 401 skips the refresh if another caller already replaced the rejected token. |
| Rejected refresh | `invalid_grant`, `interaction_required`, `consent_required`, `login_required` → grant and memory token deleted, account `reauth_required`, one notification per day, cards "Reconnect required". Never a browser from a scheduled run. |
| Provider unreachable | Network error or 429/5xx during refresh → the request fails as a network error; grant and "Connected" stay. |
| Reconnect | Replaces the grant; another account replaces the previous one (its grant and sync cursors are deleted); a refresh token of the same account is kept only when the provider issued none and the old grant was still good. |
| Disconnect | The account's last connector revokes at Google (refresh token), deletes grant and memory token; history stays. |

### 5.5 Checks after sign-in (B §47–§49)

`connectors/validate.rs`, one request per connector with the new access token:

| Connector | Request | Reads |
|---|---|---|
| Gmail | `GET gmail/v1/users/me/profile` | address and counts, no message |
| Google Calendar | `GET calendar/v3/calendars/primary/events?maxResults=1&singleEvents=true&fields=kind&timeMin=now` | only `kind` |
| Outlook Mail | `GET /me/messages?$top=1&$select=id` | one message id |
| Outlook Calendar | `GET /me/calendar?$select=id` | the calendar id |
| Microsoft identity | `GET /me?$select=id,displayName,mail,userPrincipalName` | the account |

A permission left unticked on the consent screen is known from the granted
scopes without a request (SCOPE_NOT_GRANTED → "Permission missing"). A 403
with `SERVICE_DISABLED`/`accessNotConfigured` is API_NOT_ENABLED; a mailbox
Graph cannot use is ACCOUNT_NOT_SUPPORTED; 401 is REAUTH_REQUIRED. The result
is stored on the connector (`last_error_code`, migration 0013). An
unreachable provider (network error, 429, 5xx) is logged as `skipped` and
leaves the connector connected. The sign-in fails only when the connector
the user clicked fails its check; the grant stays so that a later retry
needs nothing else.

### 5.6 Error taxonomy (B §60–§62)

`connectors/failure.rs`; codes are `ConnectorErrorCode` (TypeScript
`errorCode` on each card). Provider texts go to "Technical details" only,
scrubbed of every value ReMa sent.

| Code | When | Card |
|---|---|---|
| USER_CANCELLED | Cancel, or the user declined consent | back to `+` (declined: "access was not granted") |
| SIGN_IN_TIMED_OUT | no redirect within 5 minutes | Connection failed · Retry |
| BROWSER_UNAVAILABLE | the default browser could not be opened | Connection failed · Retry |
| INVALID_STATE | a redirect ReMa did not start | Connection failed · Retry |
| REDIRECT_MISMATCH | `redirect_uri_mismatch`, AADSTS50011 | "this copy of ReMa's registration needs an update" |
| TOKEN_EXCHANGE_FAILED | the code was not accepted, no account id | Retry |
| SCOPE_NOT_GRANTED | permission unticked or refused | Permission missing · Reconnect |
| PROVIDER_ADMIN_POLICY | AADSTS90094/90095, AADSTS65001 naming an admin, Google `admin_policy_enforced` | "Your organization requires administrator approval before ReMa can access this Microsoft account." |
| API_NOT_ENABLED | API disabled for ReMa's project | "not a problem with your account; updating ReMa fixes it" |
| ACCOUNT_NOT_SUPPORTED | no mailbox Graph can use | explained |
| REAUTH_REQUIRED | rejected refresh, 401 | Reconnect required |
| NETWORK_ERROR | unreachable, 429, 5xx | Retry; connected accounts stay connected |
| OAUTH_APP_NOT_VERIFIED | Google verification refusal | development builds: "add this account as a test user"; release builds never tell users to become test users |
| PROVIDER_CONFIGURATION_ERROR | `invalid_client`, `client_secret is missing`, AADSTS7000218/700016, `org_internal` | "not a problem with your account" |
| CREDENTIAL_STORE_UNAVAILABLE | the system keychain did not answer within 10 s | Keychain unavailable (the connection is unchanged) |

### 5.7 Diagnostics (B §59)

Written through `connectors::diag` (tests keep the lines):

```text
[connector] config google=ready microsoft=ready linkedin=off tenant=common source=connectors.toml
[oauth] provider=google phase=started connectors=gmail
[oauth] provider=google phase=listener_bound port=40517
[oauth] provider=google phase=browser_opened
[oauth] provider=google phase=callback_received
[oauth] provider=google phase=token_exchange_success
[oauth] provider=google phase=account_identified
[connector] provider=google capability=gmail validation=success
[oauth] provider=google phase=connected
[oauth] provider=google phase=token_refreshed
[connector] provider=google state=reauth_required
```

Never logged: codes, tokens, verifiers, `state`, client secrets, email
addresses, account ids, mail content.

### 5.8 Interface (B §41–§43, §63–§64)

- Cards: `+`; spinner and "Opening … sign-in…"; check and address;
  "Reconnect required" with Reconnect; "Connection failed." with the reason
  and Retry; "Last sync failed" for a failed Job Mail & Interview Sync run;
  "Keychain unavailable". OAuth details only behind "Show details".
- Details: account, Capabilities (every connector of the account and its
  state), Permissions ("Mail — Read only", "Calendar — Read events",
  "Calendar — Create and update events", "Availability — Read"), last sync,
  Sync now (runs Job Mail & Interview Sync, only while it is on), Reconnect,
  Disconnect.
- Where job mail goes (`ConnectorsOverview.mailProcessing`): with a cloud
  model, "Job-related email text is sent to Anthropic (claude-sonnet-5) to
  be read; other mail is filtered on this computer and never sent."; with a
  model on this computer, "… mail leaves it only between ReMa and Google or
  Microsoft." ReMa never claims mail stays on the device when it does not.
- Settings reads the keychain only for connected accounts (a fresh install
  reads none) and waits at most 10 s for it.

### 5.9 Found in the packaged-release run (validation §4.2)

- **Chat asked the web about the user's own mail.** "Which job emails did I
  get?" reads like a job search, so ReMa ran a public listing search. A
  question about the user's own mail, calendar or applications is now
  answered with the connector tools and without the web, unless it names
  listings ("Find jobs like my applications" is still a search).
- **A mailbox that failed inside a run was marked synced.** With Gmail and
  Outlook connected, a run that read only Gmail recorded both as
  successful: Outlook's error was cleared, its card said "Synced", and its
  last success moved forward — the point where a recovery after an expired
  cursor starts reading, so mail from the failed period could be skipped.
  `jobs::run` now returns the mailboxes it could not read; each keeps its
  last success and shows its own error.
- **An unusable Outlook cursor stopped Outlook sync for good.** A stored
  delta link that is not Graph's is still never followed (the token never
  leaves), but it now restarts a bounded synchronization like an expired
  one, instead of failing every run.
- **A run that read no mailbox said "Succeeded".** With every provider out
  of reach, Job Mail & Interview Sync reported success, and the calendar
  step marked the calendars "Synced" although no request to them worked. A
  run that reads none of its mailboxes now fails with the first mailbox's
  error (each mailbox keeps its own), calendars count as checked only when
  every request to them worked, and an unreachable provider reads "The
  provider could not be reached. Check your connection; the next run tries
  again." The grants stay (B §77).

### 5.10 Not done by design

- **No MSAL.** Microsoft has no supported MSAL for Rust; ReMa implements the
  documented authorization-code + PKCE flow for public clients (no secret,
  `offline_access`, `localhost` redirect) and is covered by the tests above.
- **No cloud auth proxy, no device code, no embedded login** (B §83–§85).
- **One account per provider** for now; keys and cursors are per account
  (B §69).
