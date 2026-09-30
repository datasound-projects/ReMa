# Connectors, search and MCP: third pass (fixes per open issue)

Follows [open-issues.md](open-issues.md), which lists the state after the
second pass ([verification.md](verification.md)). This pass worked every
ID there. One row per ID, then what remains for the owner. The fourth
pass, the connection runtime (authentication belongs to ReMa, not to a
model), is at the end: [Connection runtime](#connection-runtime-fourth-pass).

**Code status:** *fixed* (changed in this pass), *unchanged* (a deliberate
limitation, kept), *unsupported* (not offered, and said so), *blocked*
(nothing to change in code; the item needs an input ReMa cannot supply).

**Verification status:** *VERIFIED* only for the claimed live integration
or platform, run for real; *PARTIALLY VERIFIED* for ReMa's own code run end
to end against stand-ins or with incomplete coverage; *BLOCKED* for a
missing prerequisite. A mock never makes anything VERIFIED.

Environment of this pass: the same Linux container as before (no macOS,
no Windows, no provider keys, no OAuth registrations or test accounts;
`api.openai.com`, `graph.microsoft.com`, the job boards and the Google,
Microsoft, MCP-specification and Devin documentation hosts are blocked by
the egress policy; Anthropic's documentation was reachable and was
re-read). Session credentials were never used.

## Checks run

| Check | Result |
|---|---|
| `cargo fmt --check`, `cargo clippy --all-targets --locked -- -D warnings` | clean |
| `cargo test --locked` (lib, integration test `build_support`) | see the summary at the end |
| `pnpm lint`, `pnpm typecheck`, `pnpm test` | clean; 146 passed |
| Live tests without keys (`cargo test --lib live_ -- --ignored`) | each prints `BLOCKED …` naming the variable |

## The design change behind B3–B6, P4, P12, L2, L3, L5

Before: one request held the web search and the connector tools; a
private read switched the search off mid-answer (an edited tool list, a
thinking block bound to the old one), and a chat that had read private
data lost the web for good.

Now: an answer is written in up to two steps
(`src-tauri/src/services/chat.rs`, `generate`).

1. **Research.** The model sees the *public view* of the chat (the user's
   own messages and answers written without private data), its own web
   search and ReMa's public tools (ReMa MCP, career, Network Connect,
   Business). The connector tools are declared so the model can ask for
   them; a call to one ends the step before anything of that step runs
   (`ChatRequest::deferred`, `Finish::Deferred`; a search asked for in the
   same step is reported as not run). The executor refuses them too
   (`PhaseGate`). A question only about the user's own data skips this step.
2. **Private step.** A separate request with the whole chat, the connector
   tools, the user's MCP servers (every call approved once private data was
   read; ReMa MCP, which searches, is not offered), the research's text and
   sources in the system prompt, and no web tools at all (`gate_web_tools`).
3. Every message records which view it was written in (migration 0017,
   `messages.context`; messages from before count as private from the first
   private read on). A later question researches again from the public
   view, in which private answers are replaced by a note.

The same flow serves every provider, so no provider's history is edited
mid-answer: Anthropic never sees a held search resumed with a private
result; OpenAI never replays reasoning items with a changed tool list;
Gemini and Codex no longer need the "connectors or search" exclusion.
Codex: a deferred tool call is answered, the turn interrupted, and the
private step runs in a new thread with `web_search` disabled (its shell,
browser and other tools are off: `accounts/codex.rs` `DISABLED_FEATURES`).

## Rows

| ID | Change or decision | Code | Verification | Evidence | Remaining dependency |
|---|---|---|---|---|---|
| B1 | Renewal hardened (one refresh per provider, retries on passing trouble, the stored refresh token kept, a disconnect during a refresh saves nothing); Google's `invalid_grant` no longer traced to Testing by age; Testing context only when the build says so (estimated end date); Google client secret optional at build and in token requests. | fixed | BLOCKED (live) / PARTIALLY VERIFIED (ReMa's code) | `connectors::tests`: `concurrent_requests_share_one_refresh`, `passing_trouble_at_the_token_endpoint_is_retried_and_never_ends_the_connection`, `a_refresh_in_flight_during_a_disconnect_restores_nothing`, `google_refreshes_without_a_client_secret_when_the_build_has_none`, `a_reconnect_says_why_google_ended_the_connection_and_whose_setting_it_is`, `a_testing_google_app_says_when_the_sign_in_ends` | Google Desktop client ID (and the secret if the registered client demands one), a test user, a person to sign in; Google publication and verification for the 7-day limit |
| B2 | Same renewal hardening; AADSTS mapping corrected (65001 consent required, 50133 session ended, 70043 sign-in frequency policy, inactivity without a promised 90 days); personal accounts get their own availability from `calendarView` (paginated, cancelled and free events skipped) since `getSchedule` is not for them; disconnect stays local (never `revokeSignInSessions`). | fixed | BLOCKED (live) / PARTIALLY VERIFIED | `a_personal_account_gets_its_own_availability_from_the_calendar_view`, `microsoft_disconnect_deletes_local_tokens`, `an_unused_microsoft_connection_says_it_expired_from_inactivity`, `db::connectors::tests::reauth_causes_keep_their_stored_names` | Entra public client ID, personal and work test accounts (admin consent where needed), a person to sign in, a network that reaches `graph.microsoft.com` |
| B3 | Two-step answers (above); nothing edited out of the Responses API history mid-answer. Live test requires the exact model (`REMA_LIVE_OPENAI_MODEL`) and runs forced and by-choice searches. | fixed | BLOCKED (live) | `services::chat::tests` (below), `llm::tests_http::openai_uses_the_responses_api_with_web_search` | `REMA_LIVE_OPENAI_API_KEY`, `REMA_LIVE_OPENAI_MODEL`, a network that reaches `api.openai.com` |
| B4 | Codex: research thread with web search, private step in a thread with `web_search` disabled and no shell/browser tools; a deferred tool call answers the runtime and interrupts the turn. | fixed | BLOCKED (live) | `accounts::codex` tests (thread config, `web_search="disabled"` for requests without web); `services::chat::tests::gemini_answers_research_first_and_then_use_the_connectors` (same orchestration path) | `REMA_LIVE_CODEX_HOME` (a Codex home signed in with ChatGPT), `REMA_LIVE_CODEX_MODEL` |
| B5 | Two-step answers: no held search is ever resumed with a private result, so `drop_block` is no longer needed in chat. Web search and fetch are asked for as direct calls (`allowed_callers: ["direct"]`) on every version that takes the field (no code execution container, ZDR-eligible). The documented prefix-mismatch refusal is handled once with `thinking-binding-controls-2026-08-01` + `drop_block`, then with thinking left out. New live test `live_anthropic_answer_in_two_steps` runs the whole chat flow against real Claude with a tracked application carrying a secret and an instruction to search for it. | fixed | BLOCKED (live) / PARTIALLY VERIFIED (HTTP level) | `llm::tests_http::web_search_is_called_directly_from_the_first_request`, `a_thinking_block_bound_elsewhere_is_dropped_once_then_left_out`, `a_search_held_next_to_a_private_tool_is_dropped_with_the_web` (the provider-level guard, kept), `services::chat::tests::nothing_private_reaches_a_search` | `REMA_LIVE_ANTHROPIC_API_KEY`, `REMA_LIVE_ANTHROPIC_MODEL`; a Claude sign-in for the OAuth variant |
| B6 | Two-step answers: Gemini gets its search in the research step and the connectors in the private step (no combined mode needed for that case; `includeServerSideToolInvocations` stays under `toolConfig` for search next to public functions on Gemini 3). | fixed | BLOCKED (live) | `services::chat::tests::gemini_answers_research_first_and_then_use_the_connectors`; `llm::gemini` tests | `REMA_LIVE_GEMINI_API_KEY`, `REMA_LIVE_GEMINI_MODEL` |
| B7 | Build preflight names every missing setting (required vs optional) and runtime file, never a value; the login-shell environment is read between nonce markers, time-bounded (10 s, killed and ignored after), tolerant of profile chatter, cached per session with a refresh; Windows resolves programs with `PATHEXT` natively and runs `.cmd` shims through `cmd.exe /d /s /c` with only the quoted path; the release workflow builds per target (Apple silicon and Intel separately), signs and notarizes from secrets when present, and otherwise uploads `-unsigned` artifacts with an `UNSIGNED.txt` marker; `tauri.conf.json` sets `minimumSystemVersion`, `hardenedRuntime` and the Windows digest/timestamp; `fetch.mjs --check` verifies fetched runtimes (checksum, format, processor, execute bit). | fixed | BLOCKED (macOS, Windows runs) / PARTIALLY VERIFIED (logic) | `tests/build_support.rs` (5 tests), `accounts::locate` tests: `a_chatty_profile_does_not_get_into_the_environment`, `a_shell_that_does_not_answer_is_stopped_and_ignored`, `windows_programs_take_the_launchable_pathext_extensions_in_order`, `cmd_wrappers_run_through_cmd_exe_with_the_quoted_path_only`; YAML and JSON validated | A Mac and a Windows PC to run the packaged app (Finder, Dock, login item, Start menu); Apple Developer ID and notarization credentials; a Windows certificate or Azure Trusted Signing |
| B8 | Nothing new to change in ReMa for the call itself; schema portability widened (L6, L7) so clients' validators accept every tool. | blocked | BLOCKED (model call) / VERIFIED (Inspector, second pass) | second pass: Inspector `tools/list`, `source_status`, `--strict` clean | Model credentials in Claude Code, Codex (ChatGPT sign-in), Claude Desktop, Cursor, VS Code, Windsurf on a machine that has them |
| B9 | Remotive results labelled as coming from its public API, which lags its site; the rest was already right (403/challenge → `SearchOnly`, never "opened" or "expired"; canonical URL identity before fuzzy duplicates, which are marked, never merged; Greenhouse `first_published` for the posting date, `updated_at` kept apart). | fixed (label) / unchanged (verified as already correct) | BLOCKED (live sources) | `retrieval::listings::tests::marks_pages_it_could_not_open`, `rema_mcp::adapters::greenhouse` tests, engine duplicate marking | A network that reaches the job boards (or the Mac) |
| P1 | Per-chat toggles unchanged; the account's identity and the chat's choices survive a reconnect. | unchanged / fixed (test) | PARTIALLY VERIFIED | `reconnecting_the_same_account_keeps_its_identity_and_choices`, `each_chat_chooses_its_connectors_across_restart_reauth_disconnect_and_reconnect` | B1, B2 |
| P2 | A forced renewal is proven to send exactly one token request with no Authorization header and no token in any log or notification. | fixed (test) | PARTIALLY VERIFIED (stand-in endpoints) | `connected_accounts_are_kept_signed_in_without_reading_anything` | A real run behind a logging proxy, or the providers' account activity |
| P3 | Age-based Google Testing inference removed; messages say only what the provider said, with the Testing context and an *estimated* end date only when the build says `testing`; Microsoft causes corrected and two new ones added with backwards-compatible stored names. | fixed | PARTIALLY VERIFIED (documentation-based mapping) | `a_refused_renewal_is_traced_to_its_cause`, `reauth_causes_keep_their_stored_names`, `src/lib/connectorNotes.test.ts` | Real error bodies from B1 and B2 to confirm the wording |
| P4 | Superseded by the two-step design (see B5). | fixed | PARTIALLY VERIFIED (HTTP level) | as B5 | as B5 |
| P5 | "Check now" reports each kind of evidence on its own (provider search, citations, a page ReMa could read, the answer's version agreeing with that page); a site that declines robots leaves the page *unverified*; targets configurable (`REMA_SEARCH_PROOF_TARGET`, three authoritative defaults); forced (capability) and by-choice runs kept separate in the live tests; a page's text is read without markup. | fixed | PARTIALLY VERIFIED (unit tests) | `career_search::proof::tests::only_what_the_provider_reported_counts`, `finds_version_numbers_and_readable_text` | The live runs (B3–B6) |
| P6 | Unchanged (Claude Code adds and connects; a call needs a model). | blocked | PARTIALLY VERIFIED | second pass | as B8 |
| P7 | Unchanged. | blocked | PARTIALLY VERIFIED | second pass | as B8 |
| P8 | A missing settings file is reported as "not installed" rather than unreadable; Windsurf's path kept (its documentation now lives with Devin and names no other path ReMa can check). | fixed | PARTIALLY VERIFIED | `mcp::import::tests::an_app_without_a_settings_file_is_not_installed_rather_than_unreadable` | The apps themselves (B8) |
| P9 | Shell discovery bounded and chatter-tolerant (B7); explicit MCP `env` still wins over inherited values; proxy and CA variables preserved. | fixed | VERIFIED on Linux (second pass) / BLOCKED on macOS | `accounts::locate` tests above; second pass packaged run | A Mac |
| P10 | Fallback to HTTP+SSE only on a plain 400/404/405 to the Streamable HTTP POST (never on connection failures, timeouts, 5xx, 401/403 or JSON-RPC errors); a server can be marked `sse` (migration 0018; the form's third transport, imports of `"type": "sse"`) and is then opened without probing; messages before `endpoint` are kept in order up to 64, more refuses the server with a named reason (never a silent drop); a lost stream fails the waiting calls with a named reason and the next use reconnects with a bounded backoff (1 s, 2 s, 4 s) without repeating the failed call, stopped by disconnect; a 401/403 on the stream names the token option and says OAuth is not supported on that transport (also refused at the form); other origins still refused; WebSocket still unsupported. | fixed | PARTIALLY VERIFIED (mock servers) | `mcp::client::tests::only_a_plain_400_404_or_405_makes_a_legacy_candidate`, `mcp::legacy_sse::tests`: `a_server_marked_as_http_sse_is_opened_without_probing_streamable_http`, `a_few_messages_before_the_endpoint_are_kept_and_a_flood_is_refused`, `a_legacy_server_asking_for_authentication_names_the_token_option`, `a_lost_stream_fails_calls_and_the_next_use_reconnects_with_backoff`, `disconnecting_stops_a_reconnection_in_progress`, `a_server_that_cannot_be_reached_is_not_tried_as_a_stream`; `services::mcp::tests::saves_and_tests_servers_on_the_older_http_sse_transport`; the second pass's real Python SDK server | A real OAuth-protected SSE server, should one matter (unsupported by design) |
| P11 | Unchanged. | unchanged | PARTIALLY VERIFIED | second pass | B9 |
| P12 | Superseded by the two-step design: reasoning items are replayed only within a step, never across the public/private boundary. | fixed | PARTIALLY VERIFIED | `services::chat::tests::connectors_are_declared_in_both_steps_and_the_web_only_in_research` | as B3 |
| L1 | Loading placeholders for every Settings section (providers, connectors, career search, MCP), a "Try again" on a failed load, and a console note naming any load slower than 2 s; credential-store reads were already off the UI thread (`spawn_blocking`) and bounded by a keychain timeout. | fixed | PARTIALLY VERIFIED (component tests) | `src/components/ui/LoadingRows.test.tsx` | A packaged run to see the timings |
| L2 | Superseded by the two-step design: Codex and Gemini answers get research and the connectors, in that order. | fixed | PARTIALLY VERIFIED | `gemini_answers_research_first_and_then_use_the_connectors` | as B4, B6 |
| L3 | Replaced: web access is off for the rest of the *answer* once private data is read, not the chat; later questions research from the public view. | fixed | PARTIALLY VERIFIED | `a_later_question_researches_from_the_public_view_after_a_private_answer`, `rema_mcp_is_not_in_the_private_step_and_research_hides_private_answers`, `nothing_private_reaches_a_search` | as B5 |
| L4 | Kept: a chat with its own connector list keeps later-connected accounts off; the **+** menu now says so. | unchanged (explained) | PARTIALLY VERIFIED | `src/components/chat/ChatToolsMenu.test.tsx` | none |
| L5 | No longer applies in chat (no mid-answer tool change); the documented one-time recovery stays for any other prefix mismatch. | fixed | PARTIALLY VERIFIED | `a_thinking_block_bound_elsewhere_is_dropped_once_then_left_out` | as B5 |
| L6 | Rust number `format`s (`uint32`, `int64`, `double`, …) removed recursively from ReMa MCP's schemas, bounds kept; nullable outputs stay `anyOf`. | fixed | PARTIALLY VERIFIED | `rema_mcp::server::portability::tool_schemas_use_one_type_per_value` | none |
| L7 | Every tool schema ReMa sends to models (career, Network Connect, Business, ReMa MCP) is checked for one type per value, standard formats, `properties` on objects and draft 2020-12 keywords; strict mode is not used (`strict: false`), so no closing is needed. The connector tools' schemas are built from private functions and are covered by their own `deny_unknown_fields` parsing rather than this walk. | fixed | PARTIALLY VERIFIED | `mcp::schema_checks` (3 tests) | none |
| L8 | Live tests require `REMA_LIVE_<PROVIDER>_MODEL` (checked against the provider's list); nothing is picked by substring; each check is one request with a 2,000-token answer. | fixed | VERIFIED (they print `BLOCKED` naming the variable) | `llm::live_tests` | the keys |
| L9 | The user's Mac has a debug build without IDs; nothing to change in code. | blocked | BLOCKED | — | rebuild with the IDs: `GOOGLE_DESKTOP_CLIENT_ID=… MICROSOFT_PUBLIC_CLIENT_ID=… GOOGLE_PUBLISHING_STATUS=testing pnpm build:app` |
| L10 | Unchanged: imported exports only, labelled as such; no partner API bypass. | unsupported (by policy) | VERIFIED (second pass, imports) | second pass | LinkedIn partner approval; XING has no API |
| L11 | Windows-native program lookup and shim launching added (B7); nothing run on Windows. | fixed | BLOCKED | `windows_programs_take_the_launchable_pathext_extensions_in_order`, `cmd_wrappers_run_through_cmd_exe_with_the_quoted_path_only` | A Windows PC |
| L12 | Signing and notarization configured from secrets, unsigned artifacts marked; the Google secret no longer blocks a build. | fixed | BLOCKED (a real run of the workflow) | `.github/workflows/release.yml` validated; `docs/handbook.md` "Building and signing" | Certificates and credentials (owner) |

## Owner checklist

Only inputs ReMa cannot supply, with the exact steps.

1. **Provider keys, for the live proofs** (from `src-tauri/`; each check is one request):

   ```sh
   REMA_LIVE_ANTHROPIC_API_KEY=… REMA_LIVE_ANTHROPIC_MODEL=<exact id> \
   REMA_LIVE_OPENAI_API_KEY=… REMA_LIVE_OPENAI_MODEL=<exact id> \
   REMA_LIVE_GEMINI_API_KEY=… REMA_LIVE_GEMINI_MODEL=<exact id> \
   REMA_LIVE_CODEX_HOME="$HOME/Library/Application Support/cloud.datasound.rema/runtimes/codex" REMA_LIVE_CODEX_MODEL=<exact id> \
     cargo test --locked --lib live_ -- --ignored --nocapture --test-threads 1
   ```

   `live_anthropic_answer_in_two_steps` is the whole-flow proof for B5/L3.
   `REMA_SEARCH_PROOF_TARGET=www.python.org` (or `nodejs.org`) switches the
   search proof's question when `blog.rust-lang.org` is unreachable. On this
   cloud environment, `api.openai.com`, `chatgpt.com` and `auth.openai.com`
   must also be allowed in its network settings.
2. **Google:** a Cloud project with the Gmail and Calendar APIs, an External
   consent screen with the tester as test user, a Desktop OAuth client. Build
   with `GOOGLE_DESKTOP_CLIENT_ID` (and `GOOGLE_DESKTOP_CLIENT_SECRET` only if
   Google's token endpoint answers `client_secret is missing` without it) and
   `GOOGLE_PUBLISHING_STATUS=testing`. Sign in on the Mac; run the checks in
   open-issues.md B1. Publication and restricted-scope verification remain
   Google's process.
3. **Microsoft:** an Entra app registration (any org + personal accounts,
   "Mobile and desktop" platform with `http://localhost`, public client
   flows, delegated `User.Read`, `Mail.Read`, `Calendars.ReadWrite`,
   `offline_access`). Build with `MICROSOFT_PUBLIC_CLIENT_ID`. Sign in with
   one personal and one work account; on a network that reaches
   `graph.microsoft.com`.
4. **A Mac:** `pnpm build:app`, install, start from Finder, the Dock and a
   login item; add MCP servers that use nvm's `node` (`npx`) and `uvx`; check
   Settings → MCP shows them connected. Then, in Claude Code, Claude Desktop,
   Codex, Cursor and VS Code, add ReMa from Settings and ask the model to
   call `source_status` and `search_jobs`.
5. **A Windows PC:** the same, from the Start menu; an MCP server installed
   through npm (a `.cmd` shim).
6. **Signing:** an Apple Developer ID certificate and notarization
   credentials, a Windows certificate or Azure Trusted Signing, set as the
   secrets named in `.github/workflows/release.yml`; then run the workflow.
7. **Job boards:** allow `www.arbeitnow.com`, `boards-api.greenhouse.io`,
   `api.lever.co`, `remotive.com` (or run on the Mac) and ask a chat for
   jobs; run Settings → Career Search → Check now.


## Connection runtime (fourth pass)

The request: ReMa authenticates Google, Microsoft and remote MCP servers
itself, like Claude Desktop and the ChatGPT app; models receive tool
definitions and sanitized results, never a token. Architecture and the
state machine: [../connectors/implementation.md §6](../connectors/implementation.md#6-connection-runtime-model-independent-authentication).

**Status vocabulary for this pass.** *Code-complete*: implemented and
covered by deterministic tests against mock providers (a mock never
verifies a provider). *Live-verified*: run for real against the provider,
with the evidence named. *External-policy-dependent*: correct in ReMa and
still gated by something only the provider or the publisher decides.

### What changed

| Area | Change | Status |
|---|---|---|
| Public configuration | Client IDs are public application configuration: compiled in (`src-tauri/connectors.toml`, release builds fail without Google and Microsoft), and every build also reads `<data dir>/connectors.toml` (validated by key, never overriding the build). A copy built without a registration offers **Set up** on the card: the registration is entered in the app (`connectors/registrations.rs`; the Google secret goes to the keychain) and the sign-in works at once. | code-complete (`public_configuration_can_come_from_the_data_folder_in_development`, `a_registration_entered_in_settings_makes_the_sign_in_work`) |
| Account cards | Settings → Connectors shows one card per provider: Connect → browser → email, ✓ Gmail ✓ Google Calendar, Connected, Manage, Disconnect (revokes at Google, deletes the Microsoft grant). Manage adds or removes each capability, lists permissions, health (sign-in time, last renewal), last sync, Sync now. LinkedIn and XING keep their cards. | code-complete (`connecting_a_provider_asks_for_every_capability_in_one_sign_in`, `ConnectorsSection.test.tsx`) |
| State machine | `ConnectionState`: disconnected, connecting, connected, refreshing, reauth_required, permission_denied, admin_approval_required, provider_error, offline, unavailable; derived per account with the structured error code and an actionable message; tenant/admin-consent rejection (AADSTS90094/65001) is `admin_approval_required`, never a ReMa failure. | code-complete (`the_account_state_machine_names_each_situation`) |
| Token lifecycle | `connect`, `disconnect`, `get_valid_access_token`, `refresh_access_token` (after a 401, the request retried once), `revoke_connection`, `requires_reauthentication`; the daily keep-alive is gone: renewal is lazy, plus one renewal for a grant unused 30 days (`renew_unused`, recorded in `connector_accounts.last_refreshed_at`, migration 0020). | code-complete (`unused_grants_are_renewed_for_a_reason_without_reading_anything`, `access_tokens_are_refreshed_silently_before_they_expire`, `a_revoked_grant_requires_reconnecting_and_never_retries_on_its_own`) |
| Chats | Account-level connections; per-chat toggles unchanged; new setting "Add new accounts to chats that chose their own connectors" (off by default). Switching the model changes nothing: `ConnectorTools::prepare` takes no model, only the chat's toggles. | code-complete (`a_new_account_joins_chats_with_their_own_connector_choice_only_when_asked`) |
| Tool router | `services/tool_registry.rs`: canonical ids (`mail.search` with the aliases `gmail.search`/`outlook.search`, `calendar.*`, `applications.*`, `mcp.<server>.<tool>`) mapped onto the stable model-facing names; provider adapters translate `ToolSpec` only. Model-facing names were not renamed (conversation history and provider histories carry them). | code-complete (`services::tool_registry::tests`) |
| Remote MCP OAuth | A server added by URL alone is probed: a 401 challenge turns it into an OAuth server with "Authentication required" and a Sign in button (no token to paste); Test in the dialog says so and switches the form to OAuth. Sign-in: protected resource metadata → authorization server metadata → pre-registered / Client ID Metadata Document (`REMA_MCP_CLIENT_METADATA_URL`, HTTPS, when the server advertises support) / Dynamic Client Registration → PKCE S256, state, loopback, RFC 8707 `resource` → token → the connection and its tools/list run. Refresh on expiry, `reauth` on a rejected refresh, a 401 during use marks the server "Authentication required" (never a browser on its own), issuer mismatch and malformed metadata stop the sign-in with a plain message. | code-complete (`mcp::oauth_tests`: 4 tests against a mock MCP + authorization server) |
| Local MCP | Unchanged: stdio servers run with ReMa's controlled environment (`accounts::locate`: login shell PATH, nvm/volta/uv/pipx paths, Windows PATHEXT). Independent of remote OAuth. | code-complete (earlier passes) |
| Web search | Unchanged and decoupled: provider-native search uses the model provider's own credentials; connector auth is the account's. The two-step answer keeps web search out of a step that read private data. | code-complete (earlier passes) |
| Live smoke tests | `pnpm test:live:google`, `pnpm test:live:microsoft` (connect → renew → one mail search → one calendar read → disconnect), `pnpm test:live:mcp` (probe → sign in → tools/list → tools/call → restart). Interactive, in-memory credential store, skip with `BLOCKED …` when their variable is not set. | code-complete; **not live-verified** (no client IDs, no accounts, no MCP server reachable from this environment) |

### Live verification: not done, and what it needs

Nothing in this pass is live-verified. The definition of done (build on
macOS → Connect Google → authorize → Connected → Claude reads Gmail →
switch to OpenAI, Gemini → restart → expired tokens renew → Disconnect →
Outlook the same → an OAuth MCP server by URL → restart) needs the owner
inputs below and a Mac. Until those runs happen, the rows above are
code-complete only.

1. **Google Cloud (once, by the publisher).** Project → APIs & Services →
   Library: enable *Gmail API* and *Google Calendar API*. Google Auth
   Platform → Branding: app name, support e-mail, logo, home page, privacy
   policy, terms, authorized domain, developer contact. Audience:
   *External*; add test users while in Testing. Data access (scopes):
   `openid`, `email`, `profile`, `https://www.googleapis.com/auth/gmail.readonly`,
   `https://www.googleapis.com/auth/calendar.events`,
   `https://www.googleapis.com/auth/calendar.freebusy`. Clients → Create
   client → *Desktop app* (no redirect URI: loopback is accepted). Put the
   client ID into `src-tauri/connectors.toml` `[google] desktop_client_id`
   (or `GOOGLE_DESKTOP_CLIENT_ID` in the release workflow); the secret only
   if the token endpoint answers `client_secret is missing`;
   `GOOGLE_PUBLISHING_STATUS=testing` until published.
2. **Microsoft Entra (once).** App registrations → New registration: name
   ReMa; supported account types *Accounts in any organizational directory
   and personal Microsoft accounts*. Authentication → Add a platform →
   *Mobile and desktop applications* → redirect URI `http://localhost`;
   *Allow public client flows* = Yes. No client secret. API permissions →
   Microsoft Graph → Delegated: `openid`, `profile`, `email`,
   `offline_access`, `User.Read`, `Mail.Read`, `Calendars.ReadWrite`.
   Branding: publisher domain, publisher verification. Put the Application
   (client) ID into `[microsoft] public_client_id`
   (`MICROSOFT_PUBLIC_CLIENT_ID`), tenant `common`.
3. **Live tests** (from the repository root, a browser opens; a person
   signs in):

   ```sh
   REMA_LIVE_GOOGLE_CLIENT_ID=….apps.googleusercontent.com pnpm test:live:google
   REMA_LIVE_MICROSOFT_CLIENT_ID=<guid> pnpm test:live:microsoft
   REMA_LIVE_MCP_URL=https://<server>/mcp pnpm test:live:mcp
   ```

   Without the variable each prints `BLOCKED …` and passes. The
   connector tests use an in-memory credential store; the keychain itself
   is exercised by running the app (step 4).
4. **In the app (macOS):** `pnpm build:app`, install, Settings → Connectors
   → Google → Connect → authorize → Connected with the e-mail and ✓ Gmail
   ✓ Google Calendar. Ask a chat (Claude) about recent job mail; switch to
   OpenAI, then Gemini: same tools, same answers, no new sign-in. Quit and
   restart: still connected (Keychain). Wait past an hour (or Manage →
   Reconnect is not needed): the next request renews the token silently.
   Disconnect → the Google account's connections page no longer lists ReMa
   and Keychain Access has no `cloud.datasound.rema` item for it. Microsoft
   the same with Outlook. Settings → MCP → Add → URL of an OAuth server →
   Test says "requires sign-in" → Save → Sign in → Connected with tools →
   restart → reconnects without a browser.

### External-policy-dependent

- **Google restricted scope (`gmail.readonly`)**: until the OAuth app is
  verified (CASA assessment), Google shows the unverified-app screen and
  limits the app to 100 users; while the app is in *Testing*, Google ends
  every sign-in about 7 days after it is made. ReMa says both on the card
  when the build says `testing`; it cannot code around either.
- **Microsoft tenants** may require admin consent for multi-tenant apps
  or block unverified publishers: `admin_approval_required` names the
  organization's policy; publisher verification is the publisher's.
- **Remote MCP servers** that support neither Dynamic Client Registration
  nor Client ID Metadata Documents accept only clients their operator
  registered; ReMa says so and offers a token instead. CIMD needs ReMa to
  host `client.json` at an HTTPS URL of its own domain
  (`REMA_MCP_CLIENT_METADATA_URL`), a publishing step.
- **LinkedIn** (60-day tokens, partner scopes) and **XING** (no API) are
  unchanged.

### Migration

Existing data is kept: `connector_accounts` gains `last_refreshed_at`
(NULL for every existing row, so a grant older than 30 days is renewed
once at the next check), conversations and their connector choices,
scheduled tasks, provider selections, local MCP configurations and ReMa
MCP are untouched; grants stay under their per-account keychain keys. A
server that was saved with *no authentication* and turns out to require
sign-in is switched to OAuth at its next connection and shows "Sign in".

### Checks run (fourth pass)

| Check | Result |
|---|---|
| `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` | clean |
| `cargo test --lib` | 839 passed, 0 failed, 13 ignored (the live tests among them) |
| `pnpm typecheck`, `pnpm lint`, `pnpm build` | clean |
| `pnpm test` | 174 passed |
| `pnpm test:live:google` / `:microsoft` / `:mcp` | not run: no client IDs, accounts or reachable OAuth MCP server in this environment (each prints `BLOCKED …` without its variable) |
