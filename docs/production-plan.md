# Production plan — Career Search (A) and Desktop Connectors (B)

Two specifications, kept outside the repository:

- **A** — `0db2db26-ReMa_Production_Grade_Always_On_Career_Search.md` (§1–§86)
- **B** — `05d000ff-ReMa_Production_Desktop_Connectors_Google_Microsoft.md` (§1–§93)

They are implemented one after the other, A first. B starts only after A
has passed its acceptance checks. Records:
[career-search/implementation.md](career-search/implementation.md),
[career-search/validation.md](career-search/validation.md),
[connectors/implementation.md](connectors/implementation.md),
[connectors/validation.md](connectors/validation.md).

## 1. Order and why

1. **A first.**
   - Every feature uses career search: Chat, Scheduled Tasks, agents, Jobs
     MCP, Network Connect, Business.
   - A extends the Phase 2 search layer, which is fresh.
   - A's capability layer (what each model runtime can do, local or cloud) is
     what B's Gmail cloud-model disclosure (B §63–§64) builds on.
   - A's rule that web evidence never reaches mail, calendar or other tools
     (A §64) must hold before B exposes more mail and calendar tools.
2. **B second.** It is packaging and provider work: build configuration,
   OAuth hardening, release checks. Nothing in A depends on it.

## 2. Model runtimes ReMa actually has (A §9, §21, §69)

| Connection in Settings | Transport | Authentication | Native search |
|---|---|---|---|
| OpenAI — API key | Responses API (`llm/openai_responses.rs`) | API key | hosted `web_search` |
| OpenAI — ChatGPT account | `codex app-server` (bundled Codex 0.157.1, private `CODEX_HOME`) | ChatGPT sign-in inside Codex; ReMa never sees a token | Codex `web_search` modes (`live`, `indexed`, `cached`, `disabled`) |
| Anthropic — API key | Messages API (`llm/anthropic.rs`) | API key | server tool `web_search_*` (+ `web_fetch_*`) |
| Anthropic — Claude Console | Messages API | short-lived Console token from Anthropic's `ant` CLI (the official OAuth client) | same server tools |
| Google Gemini — API key | Gemini API | API key | Google Search grounding |
| OpenAI-compatible server (Ollama, LM Studio, llama.cpp, vLLM, MLX servers, Unsloth Studio …) | Chat Completions | optional key | none, except Unsloth Studio's own `web_search` tool |

Not supported by ReMa, so not tested: the OpenAI Agents API, the Claude
Agent SDK / Claude Code runtime, and a Claude.ai consumer login. The
Claude Console connection signs in through the `ant` CLI and yields API
credentials for the Messages API. No consumer token is ever reused (A §27).

## 3. Gap analysis of A against the code (at 9ef3263)

| A § | Requirement | Before | Work |
|---|---|---|---|
| 5–6, 58, 67 | Authentication separate from capability; `ProviderRuntimeCapabilities`; probe after connecting | Capability was inferred from provider kind (`native::supported`); Unsloth probed | New capability layer per runtime; learned facts (organization disabled search, workspace policy) with expiry; metadata-only probe when a provider is added or changed and at start |
| 7 | First-run bootstrap of the search infrastructure | Parts were created lazily | `career_search::bootstrap` at start, logged, no wizard |
| 13 | Provider domain limits | 20 domains for every provider | Provider caps (OpenAI 100) applied where the request is built |
| 14–15 | Verify `web_search_call`; keep sources, annotations, ranges | Verified; ranges and the new `queries` field dropped | Parse `action.queries`, `incomplete`; citations keep their ranges and quoted text |
| 17–19 | ReMa's Codex sessions default to `web_search = "live"` | Runtime default `disabled`, `live` per thread for chat | Runtime default `live`; every thread states its mode (`disabled` for extraction and private-data answers); Codex's `tools.web_search` domain filter and location now used |
| 22–23 | Anthropic tool versions kept up to date; `allowed_callers` | `web_search_20260209` / `20250305` hard-coded | One table: `web_search_20260318` + `web_fetch_20260318` for models with dynamic filtering, `allowed_callers: ["direct"]` for models without programmatic tool calling, `20250305` otherwise; a 400 asking for `allowed_callers` is retried once direct |
| 25, 54 | Organization-disabled search → ReMa evidence | Required steps fell back; chat answered without any search | The chat answer continues with ReMa's career tools; the fact is remembered |
| 34, 37, 44–47 | Discovery → pages → extraction → chunks → ranking | Structured sources only; page text cut at N characters | `SearchDiscoveryProvider` (ReMa Jobs, company sources, DuckDuckGo HTML as one provider); shared content extractor (Business reuses it); section chunks ranked with BM25 |
| 41–42 | Scope CONTRACTS; `supports_contracts`, `ats`, `enabled` | Contracts folded into jobs | `Scopes.contracts`; registry fields; contract marketplaces |
| 48–49 | One citation model; unknown references never render as sources | Sources list per answer | `Citation` for every provider; answers are checked against the table |
| 50–51 | `valid_through`, `verified_at`, application address, source job id | Partly | Carried through listings |
| 31, 63–65 | Local tools return evidence; query privacy; no exfiltration | Tools could open any address | `rema_read_page` opens only pages ReMa found in this answer or the user named; queries are trimmed of contact data |
| 66 | `[career-search]` diagnostics | Route reports only | One log line per search, no secrets and no query text |
| 81 | Migration of `search_service` | Setting kept, optional | Verified and recorded, nothing deleted |

Already in place from Phase 2 and re-verified here: requirement and
scopes, the registry-based domain filter, the Jobs MCP as the primary job
layer, native search verification, harness-driven search for local models,
Unsloth's `web_search`, failure isolation, the honest outage message,
Settings "Career Search — Automatic", Scheduled Tasks and agents on the
same router, SSRF protection in the fetcher.

## 4. Checklist A

Verified: automated tests pass and the item was exercised in the running app
([validation §6](career-search/validation.md)). Numbers in the evidence
column are rows of the in-app table (validation §6.2).

| # | Item | Status | Evidence |
|---|---|---|---|
| A1 | Capability layer, probe, bootstrap, diagnostics | Verified | capability and bootstrap tests; rows 1, 2, 6, 16 |
| A2 | Codex live default, per-thread modes, domain filter | Verified | Codex tests; rows 3, 4, 5, 12 (real Codex binary; the code-mode search found in row 3 is fixed) |
| A3 | Anthropic tool table, `allowed_callers`, organization fallback | Verified | Anthropic and fallback tests; rows 5, 6, 8, 14 |
| A4 | OpenAI limits, `queries`, citation ranges | Verified | Responses tests; row 7 |
| A5 | CONTRACTS scope, registry fields, contract sources | Verified | registry and requirement tests (in-app contract searches need the blocked marketplaces) |
| A6 | Discovery providers (DuckDuckGo one of several) | Verified | discovery tests; row 10 |
| A7 | Content extractor, chunks, BM25 evidence | Verified | extractor and evidence tests; row 10 (banner and hidden text never reached the model) |
| A8 | Citation model and integrity check | Verified | citation tests; rows 3–9 (every row links its posting) |
| A9 | Job verification fields | Verified | `posting_facts_survive_the_merge_and_show_in_the_table`; rows 4, 12 |
| A10 | Local tools: evidence, provenance, query privacy | Verified | tool tests; row 10 (a page ReMa did not find was refused) |
| A11 | Tests (§69–§80 automated) | Verified | 684 Rust (3 ignored), 118 Vitest; mapping in validation §6.1 |
| A12 | In-app E2E matrix | Verified | validation §6.2–§6.4 |
| A13 | Docs, commit, push, CI | Done | this file, implementation §8, validation §6; CI runs on the push |

## 5. External blockers (need the user or a normal network)

What this container cannot show, and what closes each gap:

- **Live OpenAI and ChatGPT searches (A §70, §71).** No OpenAI API key or
  ChatGPT account is available, and `api.openai.com`, `chatgpt.com` and
  `auth.openai.com` are blocked. The Codex path ran with the real bundled
  Codex 0.157.1 binary against a stand-in built from Codex's own source,
  with a seeded test sign-in. Needs: one ChatGPT sign-in and one API key on
  a normal network, then the §70 and §71 prompts.
- **Live Claude search (A §72).** No Anthropic API key or Console login is
  available. The Messages requests were checked against a stand-in. Needs:
  a key, then the §72 prompt, once with an organization that has web search
  turned off.
- **Live sources.** Job boards, ATS APIs, Wikidata and DuckDuckGo are
  blocked here; ReMa's own sources and discovery ran against local
  stand-ins in their documented formats.
- **Clean machine (A §80).** Simulated with fresh data directories and a
  fresh Codex home in this container, not on a second computer.

## 6. Specification B — why the connectors were "unavailable" (B §1, §50 A–C, §93 1–4)

Reproduced with the unchanged build: started without client IDs in the
environment, Settings → Connectors showed all four cards as **Unavailable**
with "Google sign-in is not available in this build of ReMa." and
"Microsoft sign-in is not available in this build of ReMa."

| Question | Answer |
|---|---|
| Google auth path | card `+` → `connect_connector` → `connectors::connect` → `ConnectorsContext::app(Google)` → `Apps::from_build()` → `google::app()`: in debug builds `REMA_DEV_GOOGLE_CLIENT_ID` at run time, otherwise `option_env!("REMA_GOOGLE_CLIENT_ID")` (+ `REMA_GOOGLE_CLIENT_SECRET`) **at compile time** |
| Microsoft auth path | the same through `microsoft::app()`: `REMA_DEV_MICROSOFT_CLIENT_ID`, otherwise `option_env!("REMA_MICROSOFT_CLIENT_ID")` |
| What sets "unavailable" | `status_of(…, available = app(provider).is_some())` → `ConnectorState::Unavailable` with `unavailable_reason()`; `connect` returns `unavailable_error` |
| Expected configuration | compile-time environment variables on the build machine; `build.rs` only asked Cargo to rebuild when they change |
| What a production build receives | **nothing**: no file in the repository holds the IDs, CI builds debug only and sets none, there is no release workflow, and `pnpm build:app` passes only whatever the builder's shell exports. `option_env!` then compiles to `None`, silently — no warning, no failure |
| Not the cause | Cargo features (none gate connectors), Tauri capabilities (the backend opens the browser through the opener plugin's Rust API; the webview has no opener permission), platform checks (none), missing provider code (OAuth, Gmail history, Graph delta, calendars exist), packaging (the bundle carries no environment) |

Root cause: **the connectors' existence depended on the environment of the
machine that compiled ReMa, and nothing checked it.**

Second finding while reproducing: `get_connectors` read the system
keychain for every connector, even ones never connected; with an
unresponsive Secret Service the whole Connectors section stayed blank.

Provider documentation (B §86–§88): the Google and Microsoft documentation
hosts are blocked here; their content was checked through search results
(sources in [connectors/implementation.md](connectors/implementation.md)).
One correction to B §20: Google "Desktop app" clients **do** require their
client secret at the token endpoint (`invalid_request: client_secret is
missing` without it, also with PKCE), while Google treats that secret as
non-confidential for installed apps. ReMa therefore ships it as public
configuration, never as a security boundary.

## 7. Gap analysis of B against the code

| B § | Requirement | Before | Work |
|---|---|---|---|
| 5–7, 80 | Deterministic public configuration; release fails without it | compile-time environment, silently "unavailable" | `src-tauri/connectors.toml` (committed) with environment overrides; `build.rs` validates and fails release builds; release workflow |
| 2–3, 44, 65, 68 | One connector layer for every feature; one account per provider | `connectors` module in `AppState`, used by Applications, Scheduled Tasks, the calendar panel and chat tools | kept; verified |
| 8, 51–53, 85 | System browser, backend-owned OAuth, restricted opener | backend opens the URL; no webview opener permission | plus a check that only the provider's own sign-in address is opened |
| 9–12, 55 | Listener bound first, loopback only, state, one callback, timeout | bound first, 127.0.0.1:0 (Microsoft: localhost on IPv4/IPv6), state, 5 min timeout | expected path only; stale or repeated callbacks refused |
| 13 | Success and failure pages | "connected" shown as soon as a code arrived | the page is answered after the token exchange, with the specified texts |
| 14, 54 | Cancel, timeout; one sign-in per provider | a second Connect cancelled the first | a second Connect is refused while one runs |
| 27, 40, 58, 69 | Token manager: refresh, single flight, reauth; key by provider + account | refresh per provider; access tokens persisted; key `connector:<provider>` | access tokens only in memory; refresh credential under `connector:<provider>:<account id>` (moved on first use); single flight per account; no reuse of a rejected refresh token |
| 37, 60–62 | Error taxonomy, admin policy, developer diagnostics | free-text errors | `OAuthErrorKind` (USER_CANCELLED … PROVIDER_CONFIGURATION_ERROR) with actionable messages |
| 41–43 | State machine and cards | "Unavailable" for missing config; failed sign-ins reverted silently | release builds cannot lack config; ERROR with Retry after a failed sign-in; "Opening Google sign-in…" |
| 47–49 | Validate each capability after sign-in | none: tokens meant "connected" | identity + Gmail profile, Calendar events, Graph `/me`, `/me/messages`, `/me/calendar`; `permission_missing` and `API_NOT_ENABLED` told apart |
| 59 | `[oauth]` / `[connector]` diagnostics | none | phase lines without codes, tokens, secrets or mail |
| 63–64 | Restricted-data boundary, cloud-model disclosure | local prefilter exists; generic privacy line | the privacy line names where job mail goes (cloud provider or this computer) |
| 70–71 | OAuth core and storage tests | partial | full list |
| 72–79 | Real-account, clean install, restart, background, offline, revocation, scope upgrade | stand-ins in a debug build | packaged release build, clean profile, provider stand-ins at the real host names; real accounts remain a blocker |

## 8. Checklist B

| # | Item | Status | Evidence |
|---|---|---|---|
| B1 | Root cause reproduced and documented | Done | §6 |
| B2 | Deterministic build configuration, release gate, CI release check | Verified | `cargo check --release` without registrations fails; build-config and release-check tests; the package logs `[connector] config google=ready microsoft=ready` ([validation §4.1](connectors/validation.md), §4.2 row 1) |
| B3 | OAuth broker hardening, pages, concurrency, error taxonomy, logging | Verified | tests in validation §4.1; packaged rows 2, 5–7, 24 |
| B4 | Token manager (memory access tokens, per-account keys, single flight) | Verified | tests; packaged rows 3, 9, 14, 20, 23 |
| B5 | Capability validation after sign-in; scheduled runs need no browser | Verified | tests; packaged rows 2, 4, 6, 7, 14, 23 |
| B6 | UI states, details, cloud-model disclosure, keychain resilience | Verified | Vitest; packaged rows 5, 8, 12, 20, 23, 25 |
| B7 | Tests (§70–§71 and the rest) | Verified | 715 Rust (3 ignored), 121 Vitest |
| B8 | Packaged release build, clean install, E2E with stand-ins | In progress | validation §4.2; final package re-check pending |
| B9 | Docs, provider checklists (§81–§82), blockers, commit, push, CI | In progress | [connectors/registration.md](connectors/registration.md), §9 |

The packaged run found five defects, all fixed with tests that fail without
the fix ([connectors/implementation.md §5.9](connectors/implementation.md)):
chat sent a question about the user's job mail to a public job search; a
mailbox that failed inside a run was marked synced (and its restart point
moved); an unusable Outlook cursor stopped Outlook sync for good; a run
that read no mailbox said "Succeeded"; unreachable calendars were marked
synced.

## 9. External blockers for B (need the publisher, the providers or other machines)

The implementation is complete and verified against provider stand-ins; the
following cannot be done from this container:

- **ReMa's own app registrations.** A Google Cloud "Desktop app" OAuth
  client (ID and its non-confidential secret; Gmail API and Google
  Calendar API enabled) and a Microsoft Entra registration (public client,
  "Mobile and desktop applications" with `http://localhost`, accounts in any
  organization and personal Microsoft accounts, delegated `User.Read`,
  `Mail.Read`, `Calendars.ReadWrite`, `offline_access`). Their values go
  into `src-tauri/connectors.toml` or the release workflow's variables;
  until then a release build fails on purpose. Steps:
  [connectors/registration.md](connectors/registration.md).
- **Google verification** (brand, domain, privacy policy) and, for
  `gmail.readonly` (a restricted scope), the annual security assessment.
  Until both pass, Gmail works only for the project's own test users, so
  the status is **implementation complete, provider verification
  pending** — not a public Gmail rollout. Calendar scopes need
  verification but no assessment.
- **Microsoft publisher verification** (recommended: without it the consent
  screen shows "unverified"); organizations that require admin consent get
  ReMa's `PROVIDER_ADMIN_POLICY` message until an administrator approves.
- **Real accounts on clean macOS and Windows machines** (B §72–§74): this
  container runs Linux (GNOME keyring through Secret Service); the macOS
  Keychain and Windows Credential Manager paths of the same `keyring` crate
  and the platforms' default browsers were not run.
- **Signed installers.** The release workflow builds unsigned packages;
  Gatekeeper (macOS notarization) and SmartScreen (Windows code signing)
  need the publisher's certificates.
- **System tray.** The container has no tray host: the tray menu (Open,
  Run Job Mail & Interview Sync, Quit) was not clicked; §76 was run by
  closing the window with background mode on (validation §4.2).
