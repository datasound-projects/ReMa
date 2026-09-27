# Native connectors — validation report

What was run and what was observed. Environment: Linux container, Xvfb
display, D-Bus session with GNOME Keyring as the credential store. No real
Google or Microsoft registration was available, and the official
documentation hosts were not reachable from this environment, so live
sign-ins against Google and Microsoft were **not** performed; everything
below ran against local stand-ins that follow the providers' documented
request and response formats.

> This report covers the connectors change. The later Job Mail & Interview
> Sync change replaced per-connector background sync, **Sync now**, the ask
> mode and the preparation buffer described below (see
> [../applications/implementation.md](../applications/implementation.md));
> its validation is in [../applications/validation.md](../applications/validation.md).

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

- Real Google and Microsoft sign-ins, consent screens and API responses (needs the publisher's registrations; see [registration.md](registration.md)). The release package was run against provider stand-ins at the real host names (§4.2).
- Google verification and the security assessment (see [compliance.md](compliance.md)).
- The system tray menu itself (Xvfb has no tray host) and native notifications on macOS and Windows.
- macOS and Windows packages on clean machines (this container runs Linux; see [../production-plan.md §9](../production-plan.md)).

## 4. Production desktop connectors (specification B)

Specification: `05d000ff-ReMa_Production_Desktop_Connectors_Google_Microsoft.md`
(§1–§93). Implementation: [implementation.md §5](implementation.md). Root
cause, gap analysis and checklist: [../production-plan.md §6–§8](../production-plan.md).

### 4.1 Automated checks

| Check | Result |
|---|---|
| `cargo fmt --check`, `cargo clippy --all-targets --locked -- -D warnings` | clean |
| `cargo test --locked` | 715 passed, 3 ignored (explicit-only) |
| `pnpm lint`, `pnpm typecheck` | clean |
| `pnpm test` | 121 passed (23 files) |
| `pnpm build` | built |
| `cargo check --release` with no registrations | **fails**, naming `GOOGLE_DESKTOP_CLIENT_ID`, `GOOGLE_DESKTOP_CLIENT_SECRET`, `MICROSOFT_PUBLIC_CLIENT_ID` and where to put them (B §6, §80) |
| Test suite through a proxy that refuses and records every request | no request leaves the test servers (after the fixes recorded in [../career-search/validation.md §6.3](../career-search/validation.md)) |

| Spec | Tests |
|---|---|
| §70 PKCE, S256 | `pkce_matches_the_rfc_7636_example`, `gmail_signs_in_with_pkce_through_the_loopback_redirect` (verifier ↔ challenge) |
| §70 state; mismatch refused | `states_are_random_and_long`, `validates_state_and_reads_denials`, `denied_consent_and_a_forged_state_connect_nothing`, `linkedin_cancel_and_forged_state_connect_nothing` |
| §70 loopback, random port | `dual_stack_accepts_whichever_loopback_address_the_browser_uses`, the sign-in tests (`http://127.0.0.1:<port>`, Microsoft `http://localhost:<port>`) |
| §70 timeout; callback after timeout | `an_unanswered_sign_in_times_out_and_a_late_callback_finds_nothing`, `times_out_and_can_be_cancelled` |
| §70 user denied | `denied_consent_and_a_forged_state_connect_nothing` |
| §70 code exchange | `gmail_signs_in_with_pkce_through_the_loopback_redirect`, `outlook_signs_in_as_a_public_client_on_localhost` (no secret) |
| §70 duplicate callback; §12 path | `the_redirect_is_taken_once_and_only_on_its_own_path` |
| §70 parallel attempt blocked (§54) | `a_second_sign_in_is_refused_while_one_is_open` |
| §13 pages after the exchange | `the_tab_gets_its_page_only_when_the_sign_in_has_ended`, `a_failed_code_exchange_says_so_in_the_browser_and_offers_retry` |
| §71 refresh token not in SQLite, logs, frontend; access token not persisted | `tokens_live_only_in_the_credential_store` (database scan, overview JSON, debug output, `[oauth]` lines, raw keychain value) |
| §40 single flight, refresh, rotation | `concurrent_requests_share_one_refresh`, `access_tokens_are_refreshed_silently_before_they_expire`, `microsoft_refresh_tokens_rotate` |
| §69 per-account key, migration | `grants_move_from_the_provider_key_to_the_account_key` |
| §27, §78 revoked → REAUTH_REQUIRED, never a browser | `a_revoked_grant_requires_reconnecting_and_never_retries_on_its_own` |
| §77 offline keeps the connection | `offline_keeps_the_connection_and_its_grant` |
| §47–§49 checks after sign-in | `a_connector_is_connected_only_when_its_api_answers`, `a_check_the_provider_cannot_answer_leaves_the_connector_connected`, `an_outlook_account_without_a_mailbox_is_explained`, `a_permission_left_unchecked_is_reported` |
| §25, §79 union of scopes | `adding_a_second_google_connector_asks_for_the_union_of_scopes` |
| §37, §60–§62 error taxonomy | `redirect_errors_become_actionable_categories`, `token_errors_depend_on_the_phase`, `api_answers_tell_permission_from_configuration`, `messages_never_carry_what_was_sent`, `an_organization_that_requires_admin_approval_is_named_as_the_reason` |
| §57 disconnect | `disconnecting_revokes_the_grant_once_the_last_connector_is_removed`, `microsoft_disconnect_deletes_local_tokens` |
| §65 scheduled run, revoked grant | `a_scheduled_job_mail_sync_with_a_revoked_grant_asks_to_reconnect` (run failed as `connector`, card Reconnect required, one refresh attempt, no browser) |
| §6, §80 build configuration | `a_release_build_without_google_or_microsoft_fails`, `a_development_build_without_them_names_what_is_missing`, `the_environment_overrides_the_file_and_every_value_is_checked`, `google_needs_the_desktop_clients_secret`, `a_release_accepts_every_organization_and_personal_accounts`, `the_compiled_source_holds_the_values_as_string_literals`, `the_committed_file_names_every_setting` |
| §51, §80 packaging | `the_opener_plugin_is_registered_and_no_webview_can_open_addresses`, `the_tauri_npm_packages_match_the_rust_crate` |
| §41–§42 states | `a_card_opening_the_browser_says_so_before_it_asks_to_finish_there`, `a_sign_in_shows_connecting_and_can_be_cancelled`, `builds_without_an_app_registration_offer_no_sign_in`, Vitest `ConnectorsSection` (Retry, capabilities, Sync now, where job mail goes) |
| Keychain resilience | `settings_never_wait_on_the_keychain_for_cards_that_were_never_connected` |
| §66, §67, §77 per-mailbox outcome, unusable cursors, nothing read | `a_mailbox_the_run_cannot_read_keeps_its_last_success_and_says_so`, `a_run_that_reads_no_mailbox_fails_and_claims_no_sync`, `links_to_other_hosts_are_never_followed`, `delta_links_continue_and_expired_tokens_resync` (found in the packaged run, §4.2) |
| §92 connectors usable from chat | `questions_about_applications_get_connector_tools_and_no_web`, `questions_about_job_mail_read_the_connectors_not_job_listings` (found in the packaged run, §4.2) |
### 4.2 Packaged release, clean install (B §74–§79, §92)

Setup, standing in for a clean machine:

- `pnpm tauri build --bundles deb` in release mode with test registrations
  in the environment (`GOOGLE_DESKTOP_CLIENT_ID=123456789012-rematestdesktop…`,
  a test `GOCSPX-…` secret, a test Microsoft GUID); `dpkg -i
  ReMa_0.1.0_amd64.deb` installs `/usr/bin/rema`.
- ReMa started with `env -i`: a new, empty home; no repository, `.env`,
  shell exports, proxy or development variables; a private D-Bus session
  with a GNOME keyring (Secret Service) as on a desktop login.
- The provider stand-in (`scripts/e2e/mock-providers.mjs`) answers at the
  **real host names** — `accounts.google.com`, `oauth2.googleapis.com`,
  `gmail.googleapis.com`, `www.googleapis.com`, `login.microsoftonline.com`,
  `graph.microsoft.com` — over HTTPS (names mapped to 127.0.0.1, a test CA
  added to the trusted roots). Like the providers, it requires Google's
  Desktop client secret, refuses a secret from Microsoft's public client,
  verifies PKCE S256 and accepts a code once.
- The default browser (`xdg-open`) is played by a script that loads the
  sign-in address and follows the redirect back to ReMa's loopback address.

| # | Spec | Scenario | Result |
|---|---|---|---|
| 1 | §74 | Fresh install → Settings → Connectors | Gmail, Google Calendar, Outlook Mail, Outlook Calendar each show **+**; none "Unavailable". LinkedIn and XING show product wording (no developer text). Startup log: `[connector] config google=ready microsoft=ready linkedin=off tenant=common source=connectors.toml_and_the_environment` |
| 2 | §70, §74 | Gmail **+** | Browser opened `https://accounts.google.com/o/oauth2/v2/auth` with the build's client ID, `redirect_uri=http://127.0.0.1:<random port>`, `code_challenge_method=S256`, `state`, `access_type=offline`, `prompt=consent select_account`, scope `openid email profile gmail.readonly`. Exchange at `oauth2.googleapis.com/token` with `client_secret`; the stand-in verified PKCE. Gmail profile checked with the new token. Tab: "ReMa connected successfully." Card: ✓ `ana@gmail.com`. Log: `[oauth] … started / listener_bound / browser_opened / callback_received / token_exchange_success / account_identified`, `[connector] provider=google capability=gmail validation=success` — no code, token, secret or address |
| 3 | §71 | Where the grants are | The keychain holds one item per account: `connector:google:g-123`, later `connector:microsoft:m-oid`. After two sign-ins and one refresh per provider, no issued token value appears in the new home (SQLite database and WAL, webview localStorage, HSTS store) or any app log |
| 4 | §79 | Google Calendar **+** after Gmail | Authorization asked for the union: `gmail.readonly calendar.events calendar.freebusy`; Gmail and Calendar (one event, `fields=kind`) checked; still one keychain item for the account |
| 5 | §62 | Outlook Mail **+**, organization requires admin approval (`AADSTS90094`) | `login.microsoftonline.com/common/…/authorize` with `redirect_uri=http://localhost:<port>`, S256, `offline_access`. Card: "**Connection failed.** Your organization requires administrator approval before ReMa can access this Microsoft account. Ask your IT administrator to approve ReMa, or connect a personal Microsoft account." with Retry; "Show details" shows the provider's code. Tab: "ReMa could not complete authorization." Log: `phase=failed category=PROVIDER_ADMIN_POLICY` |
| 6 | §73 | Retry with consent allowed | Token request without `client_secret` (public client; the stand-in refuses one), PKCE verified; identity from Graph `/me`; `/me/messages?$top=1&$select=id` checked. Card ✓ `ana@outlook.com`; keychain item `connector:microsoft:m-oid` |
| 7 | §79 | Outlook Calendar **+** | Authorization adds `Calendars.ReadWrite` to the granted scopes; `/me/messages` and `/me/calendar` checked |
| 8 | §43–§46 | Gmail details | Account, Capabilities (Gmail, Google Calendar: Connected), "Mail — Read only", "Last sync: Not yet", "Not read automatically: turn on Job Mail & Interview Sync…", Reconnect, Disconnect, myaccount.google.com |
| 9 | §75 | Quit, start again | All four ✓ with no browser; Settings made no provider request. First use (calendar panel): Google refreshed (with the Desktop client secret) and Microsoft refreshed (public client); events from both calendars shown. Log: `phase=token_refreshed` for both |
| 10 | §65, §92 | Local model endpoint added; Job Mail & Interview Sync set up (every 15 minutes), Run now | Gmail and Outlook read (metadata first, full text only for job mail), 11 new, 9 job-related, 8 applications updated, both calendars checked for conflicts, one interview added to Google Calendar, one conflict flagged. Every request to the model: `leaked: false` — "Dinner on Sunday" and the newsletter never reached it |
| 11 | §92 | Applications | Interviews, applications and the Contoso offer (from Outlook) listed, "Updated by Job Mail & Interview Sync" |
| 12 | §64 | Where job mail goes | "Job-related email is read by mock-classifier on this computer: mail leaves it only between ReMa and Google or Microsoft. Sign-in tokens never reach the interface or a model." |
| 13 | §92 | Chat: "Which job emails did I get recently, and is tomorrow at 09:00 free for a call?" | **Failed**: ReMa ran a public job-listing search ("ReMa could not retrieve live career sources…") instead of the mail and calendar tools. Fixed (`listing_search` in `services/chat.rs`, test `questions_about_job_mail_read_the_connectors_not_job_listings`, red before the fix); re-run on a rebuilt package below |

| 14 | §76 | "Run ReMa in background" on; quit and start again (nothing in memory); close the window as a window manager does (`WM_DELETE_WINDOW`) | Window hidden, process alive, no provider request until the schedule. The 20:15 run with no window: Google and Microsoft grants refreshed (`phase=token_refreshed`), Gmail history read, both calendars checked. A second launch showed the running window (single instance) and exited |
| 15 | §66 | Outlook in the 20:00 and 20:15 runs | **Failed**: no Outlook request. The stand-in had answered the first round with a delta link on its debug address; ReMa refused to follow it (correct: the token never leaves Graph), which surfaced two defects: the run marked Outlook Mail "Synced" and moved its last success, and the unusable cursor stopped Outlook sync for good. Both fixed ([implementation.md §5.9](implementation.md), tests in §4.1); the stand-in now keeps its links on the host that was called |

Rows 16–25 ran on the package built from `964ca1a`, installed over the
previous one (same home, keychain and database):

| # | Spec | Scenario | Result |
|---|---|---|---|
| 16 | — | Package upgrade | After `dpkg -i` of the new package and a start, all four connectors ✓ with no sign-in and no provider request |
| 17 | §66 | Outlook with the unusable stored link, Run now | The link was not followed; a bounded resync went to `graph.microsoft.com` (`changeType=created&$filter=receivedDateTime ge …`). Progress "Synchronized Gmail and Outlook Mail"; note "Outlook Mail: the sync position had expired; ReMa resynchronized recent mail." |
| 18 | §66 | Next scheduled run (20:45) | Outlook read incrementally from `https://graph.microsoft.com/v1.0/me/mailFolders/inbox/messages/delta?$deltatoken=1`; Gmail from `history?startHistoryId=…` |
| 19 | §92 | Chat re-run: "Which job emails did I get recently, and is tomorrow at 09:00 free for a call?" | `mail_search` (Gmail search and metadata, Graph `$search`) and `calendar_check_availability` (both calendars); answer: "You have 10 job emails; the newest: Offer letter - Cloud Engineer; Interview confirmed - ML Engineer; Update on your application - Site Reliability Engineer. Tomorrow 09:00–09:30 is taken by "Weekly sync"." Model context: `personal_mail: false`, `sign_in_tokens: false`; no web or career-search tools |
| 20 | §77 | Quit; providers unreachable (stand-in stopped); start; calendar panel | "Google Calendar could not be read: Network error: Google could not be reached. Check your internet connection and try again." (same for Outlook); tracked interviews still shown. Log `phase=refresh_failed category=NETWORK_ERROR` for both; both keychain items kept |
| 21 | §77 | Job Mail & Interview Sync while unreachable | **Failed**: the run said "Succeeded" (progress "No mailbox could be read"), the mail cards "The last run failed", the calendar cards "Synced just now". Fixed in the final package (row 26) |
| 22 | §77 | Providers back; calendar panel; Run now | Both grants refreshed, events from both calendars; the run succeeded and all four cards show "Synced just now" |
| 23 | §78 | Quit; revoke both grants at the providers (`invalid_grant`); start; calendar panel | One refresh attempt per provider, `phase=refresh_rejected category=REAUTH_REQUIRED`; both keychain items deleted; no browser opened. Panel: "Google Calendar needs to be reconnected in Settings → Connectors." Cards: "Reconnect required: Google access was revoked or has expired." with Reconnect |
| 24 | §58 | Reconnect on Gmail, then on Outlook Mail | One sign-in per provider for all its connectors (Google asked for the union again); every capability checked; new keychain items; the cards keep their real last sync |
| 25 | §57 | Disconnect Google Calendar, Gmail, Outlook Calendar, Outlook Mail (confirmation each) | Google Calendar first: grant kept (Gmail uses it). Gmail: `POST https://oauth2.googleapis.com/revoke` and the Google item deleted. Microsoft: local items deleted, no request (the dialog says so and points to account.microsoft.com). Applications, the task and its run history stay |

Row 26 ran on the final package, built from `f3f2c52`:

| # | Spec | Scenario | Result |
|---|---|---|---|
| 26 | §74, §77 | New empty profile; then the main profile: Gmail, Google Calendar and Outlook Mail connected, quit, providers unreachable, start, Run now; providers back, Run now | New profile: all four **+**, none "Unavailable", `[connector] config google=ready microsoft=ready`. Unreachable: the run **Failed** — "Network error: Google could not be reached. Check your internet connection and try again." — Gmail and Outlook Mail "Last sync failed: The provider could not be reached. Check your connection; the next run tries again.", Google Calendar not marked synced, both keychain items kept. Back online: the run succeeded ("Synchronized Gmail and Outlook Mail"), all three "Synced just now" |

Across all sign-ins (9) and refreshes (12, including refused and failed
ones), no issued token value and no client secret appears in either home
directory (database and WAL, webview storage) or any app log; the Google
Desktop client secret is only in the binary (public configuration). After
the run the provider host names were removed from `/etc/hosts`, the
stand-ins stopped and the package removed.
