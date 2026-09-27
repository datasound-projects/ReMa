# Native connectors — validation report

What was run and what was observed. Environment: Linux container, Xvfb
display, D-Bus session with GNOME Keyring as the credential store. No real
Google or Microsoft registration was available, and the official
documentation hosts were not reachable from this environment, so live
sign-ins against Google and Microsoft were **not** performed; everything
below ran against local stand-ins that follow the providers' documented
request and response formats.

## 1. Automated checks

Rust 1.94.1 and 1.98.1 (the CI's stable) gave the same results.

| Check | Result |
|---|---|
| `cargo fmt --check` | clean |
| `cargo clippy --all-targets --locked -- -D warnings` | clean |
| `cargo test` | 446 passed, 3 ignored (pre-existing explicit-only tests: bindings export, a measurement, a live Codex check) |
| `pnpm typecheck`, `pnpm lint` | clean |
| `pnpm test` | 73 passed (15 files) |
| `pnpm build` | built (tsc -b + vite) |

New and rewritten tests:

**Connector sign-in and tokens** (`src-tauri/src/connectors/tests.rs`, 18 tests), against a mock Google and Microsoft with a fake browser that follows the loopback redirect:

- Gmail sign-in: PKCE S256 (verifier hashes to the challenge), random state, `access_type=offline`, Gmail-only scopes, code exchange with the verifier; the browser shows "ReMa connected successfully. You can close this tab and return to ReMa."
- Tokens only in the credential store: no token, code, verifier or client secret in any SQLite table, in the interface's JSON or in `Debug` output.
- A second Google connector asks for the union of scopes; an unticked permission shows "Permission missing".
- Denied consent and a forged `state` connect nothing and exchange no code; a sign-in shows "Connecting" and can be cancelled; an unanswered sign-in times out.
- Builds without a registration show "Unavailable" and never open a browser.
- Outlook: `http://localhost` redirect, public client (no secret), `offline_access`, no send/write-mail scopes.
- Silent refresh 5 minutes before expiry, forced refresh, Microsoft refresh-token rotation.
- A revoked grant: tokens deleted, "Reconnect needed", no further token requests, exactly one notification; Reconnect restores it.
- Disconnect keeps the shared token while a sibling connector is enabled, revokes at Google with the last one, deletes cursors and the account, keeps applications.
- Preferences and background-sync validation; one sync per connector; app exit cancels syncs.
- Old Google settings: built-in-client tokens keep working; user-client tokens are revoked and reconnect is asked.

**Pipeline** (`src-tauri/src/jobs/tests.rs`, 25 tests), with a fake mailbox (history cursor), fake calendar and scripted model:

- First run: prefilter → triage → classification → tracker; statuses for each case; proposed slots validated against the email and checked for availability.
- Unrelated mail is never read in full, never reaches the model (not even its subject) and keeps no subject or sender.
- Incremental syncs read only changes; an expired cursor resyncs without reprocessing.
- A follow-up in a known thread skips triage and produces "Application changed to Interview", source Gmail, confidence 98%.
- Every category maps to its status; other job-related mail only adds to a known application; ambiguous mail (confidence < 50%) changes nothing and is listed for review.
- Failed triage is retried; without a model mail waits; Outlook mail is tracked separately with its own source and ids.
- Prompt injection: an email cannot close its data block, pipeline requests carry no tools, an out-of-schema answer changes nothing; a manipulated interview time that the email does not state never reaches the calendar.
- Calendar: ask mode proposes and the user adds (then no duplicate); declined interviews are not proposed again; auto mode adds confident confirmations at free times, asks when not confident; conflicts (including the preparation buffer) block creation until "Add anyway"; reschedules update the same event, a reschedule into a conflict is reported and not moved, cancellations mark the event; a repeated confirmation, a lost event id and a user-deleted event never create duplicates; calendar off touches nothing; notifications are not repeated.

**Assistant tools** (`src-tauri/src/services/connector_tools/tests.rs`, 8 tests, mock Gmail): only connected services' tools are offered; mail search returns job mail only as marked private data; email text cannot close the data block; unrelated mail cannot be read; strict argument validation; changes wait for approval every time ("Allow for this chat" counts once). `services/chat.rs`: a question about applications gets connector tools and no web access; other questions keep the web.

**Background worker** (`services/mail_monitor.rs`): only due mail connectors with background sync start, never twice at once, and the next run is scheduled an interval later.

**Interface** (Vitest): Connectors section (7), Applications page (4), tool approvals (3), navigation (Connectors present, no OAuth client fields).

## 2. In-app end-to-end (debug build)

`scripts/e2e/mock-providers.mjs` served Google (OAuth, Gmail, Calendar), Microsoft (identity platform, Graph mail and calendar) and an OpenAI-compatible model on `127.0.0.1:8777`; `scripts/e2e/bin/xdg-open` played the system browser (it follows the provider's redirect to ReMa's loopback address). The debug build ran with `REMA_DEV_*_CLIENT_ID`, `REMA_GOOGLE_BASE_URL`, `REMA_MICROSOFT_BASE_URL` and a fresh data directory, and was driven with xdotool; screenshots were checked at each step.

The mock mailbox held five emails: an application confirmation (Acme), a job-alert newsletter, a confirmed interview five days ahead (Globex, 14:00–15:00 Europe/Vienna), a rejection (Initech) and a personal email; Outlook held an offer (Contoso).

| Step | Observed |
|---|---|
| Settings → Connectors | Four cards with icons, "by Google"/"by Microsoft", the specified descriptions, `+` on each. No client ID or secret fields. |
| `+` on Gmail | Browser opened `…/o/oauth2/v2/auth` with `code_challenge_method=S256`, a random `state`, `access_type=offline`, `prompt=consent select_account`, scopes `openid email profile gmail.readonly`. Token request: `grant_type, code, code_verifier, redirect_uri, client_id, client_secret`. Browser page: "ReMa connected successfully. You can close this tab and return to ReMa." Card: green check, `ana@gmail.com`, "Synced just now". |
| First Gmail sync | `profile` (historyId), one dated job search, metadata for 5 messages, **one** triage request, full reads only of the 3 job emails (not the newsletter, not the personal email), 3 classifications. Bell: 2 notifications. |
| `+` on Google Calendar | Scopes requested: `gmail.readonly calendar.events calendar.freebusy` (union). The following Gmail sync used `history.list` from the stored historyId; the calendar step looked for an existing ReMa event and checked conflicts. |
| Applications | Summary (3 updates, 1 interview, 3 applications); Initech Rejected, Globex Interview "Fri, Oct 2 … (Europe/Vienna)", Acme Application received. |
| Globex details | Interview "Not in calendar yet" with **Add to calendar** / **Don't add** (ask mode); timeline "Source: Gmail message · Confidence: 97%"; correspondence with "Open in Gmail". |
| Add to calendar | One `POST …/calendars/primary/events`; badge "In Google Calendar"; timeline "Interview added to Google Calendar · Source: You". |
| Bell | "Interview confirmed — … No calendar conflicts. Add it to Google Calendar in Applications.", two "Application update detected"; marked read when opened. |
| `+` on Outlook Mail | `…/common/oauth2/v2.0/authorize`, redirect `http://localhost:<port>`, scopes `openid profile email offline_access User.Read Mail.Read`, `prompt=select_account`, `response_mode=query`; token form without `client_secret`. Graph delta sync, one body read, Contoso tracked as **Offer**. |
| Gmail details | Account, permissions, last/next sync, background sync switch, Sync now (one more `history.list`), Reconnect, Disconnect. |
| Disconnect Gmail | Confirmation says history stays and Google Calendar stays connected; no revocation request. |
| Disconnect Google Calendar | Confirmation says ReMa revokes access at Google; one `POST /revoke`; applications kept. |
| Database | No `g-at-`/`g-rt-`/`m-at-`/`m-rt-` token, no authorization code, no client secret, no newsletter or personal-mail subject or content; filtered mail stored as id + domain only; Google account row removed. |
| Run ReMa in background + Start ReMa at login | `~/.config/autostart/ReMa.desktop` with `Exec=… --background`. Closing the window (WM_DELETE_WINDOW) hid it; the process kept running. |
| Second launch while hidden | Exited at once and showed the running window (single instance). |
| Both options off | Autostart entry removed; closing the window quit ReMa. |

Findings fixed during the run:
- a second launch would have started another instance syncing the same mailboxes: the single-instance plugin now hands over to the running app;
- calendar connectors showed "Last sync: Not yet" although mail syncs used them: the calendar step now records its time on the calendars it used;
- the interview time was printed twice and the detail sections were cramped.

## 3. Not verified here

- Real Google and Microsoft sign-ins, consent screens and API responses (needs the publisher's registrations; see [registration.md](registration.md)).
- Google verification and the security assessment (see [compliance.md](compliance.md)).
- The system tray menu itself (Xvfb has no tray host) and native notifications on macOS and Windows.
- Release packaging of this change.
