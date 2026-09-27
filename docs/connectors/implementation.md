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
  calendar_sync.rs     ask / auto, conflicts + buffer, dedupe, reschedule, cancel, decline
  tracker.rs           overview, detail, user and assistant changes
        │
services/mail_monitor.rs   background worker (60 s tick, per-connector interval), Sync now,
                           the scheduled "Job Application Mail Monitor"
services/connector_tools.rs  the assistant's mail, calendar and tracker tools
services/notifications.rs    stored notifications + OS notifications
services/background.rs       "Run ReMa in background" (tray), "Start ReMa at login"
db/migrations/0008_connectors.sql
```

Gmail and Google Calendar are two cards sharing one Google account (one
token); Outlook Mail and Outlook Calendar share one Microsoft account.

## 2. Flows

**Connect** (`connectors::connect`): the union of the provider's enabled
connectors plus the new one determines the scopes → PKCE pair and 24-byte
random `state` → loopback listener (Google `127.0.0.1`, Microsoft
`localhost` on IPv4 and IPv6) → the system browser opens the provider's page
→ the redirect's `state` is compared in constant time → code exchange with
the verifier → granted scopes read from the token response → tokens to the
credential store, account and connectors to SQLite → the worker is woken for
a first sync. The browser shows "ReMa connected successfully. You can close
this tab and return to ReMa." Sign-ins time out after 5 minutes and can be
cancelled.

**Sync** (`mail_monitor::run_connector` → `jobs::run`): mailbox changes since
the cursor (Gmail `historyId`, Graph `deltaLink`; expired cursors resync from
the last success, at most 90 days) → dedupe by (provider, message id) →
prefilter → headers-only triage for candidates → full read and
classification for relevant mail (≤ 25 per run) → tracker → calendar step
with the connected calendars → notifications. The cursor is stored in the
same transaction as the records it covers.

**Calendar step** (`calendar_sync::sync_interview`), for each confirmed,
upcoming interview: existing event? (stored id → private
`remaInterviewId` property → fingerprint of company, role, start and
conversation) → conflicts (with the preparation buffer) → create (auto mode,
confidence ≥ 0.85, free), propose (ask mode, the default), report a conflict,
update (reschedule to a free time), keep but mark cancelled, or respect a
deletion or a "Don't add".

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
- **Background by default for mail, not for the app.** Mail connectors sync in the background while ReMa runs (default every 15 minutes); the tray and start-at-login options are off until the user enables them. Nothing runs when ReMa is closed.
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
| Detail panel: account, status, permissions, last and next sync, Sync now, Reconnect, Disconnect with confirmation, background sync | done | `ConnectorDetail` |
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
| Conflict detection with preparation buffer; ask (default) / auto | done | `jobs/calendar_sync.rs`, preferences |
| Dedupe (provider, message, application, fingerprint, event id); reschedule updates the event | done | `db/jobs.rs`, `calendar_sync.rs` |
| AI tools mail/calendar/applications, validated in Rust; approvals for changes | done | `services/connector_tools.rs` |
| Prompt-injection guards (no tools in pipeline, fenced data, schema validation, no web next to private data, MCP approval) | done | `jobs/extract.rs`, `connector_tools.rs`, `chat_tools.rs`, `chat.rs` |
| Scheduled "Job Application Mail Monitor" | done | `services/scheduler.rs`, `mail_monitor::run_task` |
| Notifications (in app + OS) | done | `services/notifications.rs`, `NotificationBell.tsx` |
| Explicit "Run ReMa in background" and "Start ReMa at login"; no hidden service | done | `services/background.rs`, `lib.rs` (tray, close handler, autostart) |
| Configurable interval, Sync now, single flight per connector | done | `mail_monitor.rs`, `connectors/sync.rs` |
| Disconnect: stop sync, delete tokens, revoke at Google, keep history | done | `connectors::disconnect` |
| Compliance documentation | done | [compliance.md](compliance.md) |
| Tests and acceptance run | done | [validation.md](validation.md) |
| Live sign-in against real Google/Microsoft | **not done here** | needs the publisher's registrations; see validation.md |
