# ReMa: unresolved issues after the connector, search and MCP pass

> Third pass (2026-09-28, later the same day): every item below was
> worked on; the outcome per ID, the evidence and what remains for the
> owner are in [fixes.md](fixes.md). This file is kept as the record of
> the state before that pass.

A handoff for the next agent. It lists every item from the last
verification pass ([verification.md](verification.md)) that was not
solved. For each one it says why it was not solved, what is already built,
what could go wrong, and what would close it.

Written 2026-09-28. Repository `datasound-projects/ReMa`, branch
`claude/rema-foundation-setup-6gmim3`. Code as of commit `051dfd2`.

---

## 0. Read this first

**What is open.** No test fails and no crash is known. The checks are clean:

- Rust: 789 tests passed. 9 live tests are ignored; they run only on request.
- Frontend: 141 tests passed.
- `cargo fmt`, `clippy -D warnings`, `pnpm lint` and `pnpm typecheck` are clean.

What is open falls into three groups:

1. **Full blockers (B1–B9).** These could not be run at all against the
   real service or platform. The code exists and has tests with stand-ins,
   but nobody has seen it work for real.
2. **Partials (P1–P12).** ReMa's code ran end to end, but with a stand-in
   provider, only on Linux, or with a documented gap in the design.
3. **Known limitations (L1–L12).** Weaknesses or deliberate choices that
   were left as they are and should be reviewed.

**Status words** (as in verification.md):

- **VERIFIED**: run for real, against the real counterpart.
- **PARTIALLY VERIFIED**: ReMa's code ran, but a provider or platform was stood in for.
- **BLOCKED**: could not be run here.

The next agent must not mark anything VERIFIED without real execution
evidence: a mock or stand-in never counts.

### Summary

| ID | What | Type | Main reasons (see §1) |
|---|---|---|---|
| B1 | Google sign-in, Gmail, Google Calendar, live | Full blocker | RC1, RC4, RC6 |
| B2 | Microsoft sign-in, Outlook Mail and Calendar, live | Full blocker | RC1, RC2, RC4, RC6 |
| B3 | OpenAI API native web search, live | Full blocker | RC1, RC2 |
| B4 | ChatGPT sign-in (Codex runtime) native web search, live | Full blocker | RC1, RC2, RC4 |
| B5 | Anthropic native search and the mail + search + thinking answer, live | Full blocker | RC1, RC5, RC7 |
| B6 | Gemini 3 native search, live | Full blocker | RC1, RC7 |
| B7 | Packaged app on macOS (and Windows) | Full blocker | RC3 |
| B8 | Other apps (Claude Code, Codex, Claude Desktop, Cursor, VS Code, Windsurf) calling ReMa MCP with a model | Full blocker | RC1, RC3, RC4, RC5 |
| B9 | Live job search through ReMa's sources and ReMa MCP | Full blocker | RC2 |
| P1 | Per-chat connector toggles across restart, reauth, reconnect, disconnect | Partial | stand-in providers |
| P2 | Renewal never reads mail or calendars | Partial | stand-in providers |
| P3 | Reasons shown when a connection ends | Partial | built from documentation, heuristic |
| P4 | Anthropic mixed flow | Partial | request shape only |
| P5 | Native search proof ("Check now") | Partial | unit tests only; forced search |
| P6 | ReMa MCP in Claude Code | Partial | connects; no tool call |
| P7 | ReMa MCP in Codex | Partial | config only |
| P8 | ReMa MCP in Claude Desktop, Cursor, VS Code, Windsurf | Partial | config files only |
| P9 | MCP servers' environment in a packaged app | Partial | Linux only |
| P10 | Older HTTP+SSE MCP servers | Partial | gaps in the design |
| P11 | ReMa's verified search mode | Partial | stand-in sources |
| P12 | OpenAI API: web search switched off mid-answer | Partial | untested live |

---

## 1. Why so much could not be done: the environment and root causes

The previous agent worked in a **Linux cloud container**, with Xvfb and
D-Bus/GNOME Keyring. It had **no Mac, no Windows, no provider API keys, no
OAuth app registrations and no test accounts**. HTTPS went through a proxy
whose policy blocks some hosts.

Reachability, measured on 2026-09-28 with unauthenticated requests through
the environment's proxy:

| Host | Result | Meaning |
|---|---|---|
| `api.anthropic.com` | HTTP 401 | Reachable; needs a key |
| `generativelanguage.googleapis.com` | HTTP 403, "unregistered callers" | Reachable; needs a key |
| `oauth2.googleapis.com`, `gmail.googleapis.com`, `www.googleapis.com` | 404 / 401 / 401 | Reachable; needs a token |
| `login.microsoftonline.com` | HTTP 200 | Reachable |
| `graph.microsoft.com` | no connection | **Blocked by network policy** |
| `api.openai.com`, `chatgpt.com`, `auth.openai.com` | no connection | **Blocked** |
| `www.arbeitnow.com`, `boards-api.greenhouse.io`, `api.lever.co`, `remotive.com` | no connection | **Blocked** (job sources) |
| `blog.rust-lang.org` | no connection | **Blocked** (the page the search proof asks about) |
| `api.linkedin.com` | no connection | **Blocked** |

**Root causes.** Every item below refers to these.

- **RC1: missing credentials.** None were available:
  - API keys for Anthropic, OpenAI and Gemini.
  - ReMa's Google Desktop OAuth client ID and secret.
  - ReMa's Microsoft Entra public client ID.
  - Google and Microsoft test accounts.
  - A ChatGPT account for Codex.
- **RC2: network policy.** The hosts marked blocked above cannot be reached
  from this environment even with credentials.
- **RC3: no macOS and no Windows machine.** Anything that depends on
  Finder, Dock, launchd, the Keychain, Gatekeeper or a Windows runtime
  cannot be run.
- **RC4: a human must sign in.** Google, Microsoft and ChatGPT sign-in
  need a person in a browser (consent, 2FA, risk checks). The rules
  forbid automating an interactive OAuth sign-in, and providers block it
  anyway.
- **RC5: session credentials are forbidden.** The container holds
  credentials for its own infrastructure (`CLAUDE_CODE_*`, `GH_TOKEN` and
  similar). Using them to call models or run tests is forbidden, so
  "just use the session's Claude access" is not an option.
- **RC6: third-party policy.** Some limits are decided by Google,
  Microsoft, LinkedIn or XING, not by code. Examples: Google's 7-day
  refresh tokens in Testing, restricted-scope verification, tenant admin
  consent, LinkedIn partner-only APIs, and XING having no API.
- **RC7: provider behaviour only a live call can confirm.** The request
  shapes follow current documentation. Whether the provider accepts them in
  every case (betas, OAuth tokens, history edits) is known only after a
  real request.

---

## 2. Full blockers

Each blocker below covers what should have been proven, why it could not
be, what exists already, the risk if it is wrong, and what closes it.

### B1. Live Google: sign-in, Gmail, Google Calendar

**Status:** BLOCKED. **Root causes:** RC1, RC4, RC6.

**What should have been proven:**

1. A real Google account connects once.
2. Gmail and Calendar tools work in chat.
3. A chat's toggles switch them on and off.
4. The connection survives a restart and renews its token.
5. Disconnect and reconnect work.

**Why it could not be done:**

1. **No OAuth client.** ReMa signs in with its own Google "Desktop app"
   OAuth client (installed-app flow: PKCE plus loopback redirect,
   `src-tauri/src/connectors/google/`).
   - The client ID and secret were not available. `src-tauri/connectors.toml`
     holds empty values, and the Actions variables
     `GOOGLE_DESKTOP_CLIENT_ID` and `GOOGLE_DESKTOP_CLIENT_SECRET` are not
     readable by an agent.
   - Without them, `build.rs` refuses a release build, and a debug build
     shows Google as unavailable.
2. **No test account**, and a person must sign in (RC4).
3. **Google's policy (RC6), which no code can change.**
   - While the Google Cloud project's OAuth consent screen is "External"
     and in **Testing**, Google ends refresh tokens **7 days** after they
     are issued. Users must reconnect every week.
   - The only fix is to publish the app ("In production").
   - ReMa asks for `gmail.readonly`, a **restricted** scope, and
     `calendar.events` and `calendar.freebusy`, **sensitive** scopes.
     Publishing with restricted scopes requires Google's OAuth app
     verification and, for restricted scopes, a yearly independent
     security assessment. Until then users see the "unverified app"
     screen and the app has a user cap.
   - This is an owner or business task, not a code task. Check the current
     Google Cloud documentation for the exact terms.

**What is already built:**

- Build key `GOOGLE_PUBLISHING_STATUS` (`testing`, `production` or empty),
  in `connectors.toml` under `[google] publishing_status` or as an
  environment variable.
- In a Testing build, the Google cards say when Google will end the
  sign-in (`sign_in_ends_at`).
- When a renewal is refused, the cause is stored (migration `0016`) and
  shown on the card and in the notification: Testing expiry, revoked,
  inactive, security policy, blocked or token lifetime.
- Tests use stand-in endpoints (see P1–P3).

**Risk if it is wrong:** real Gmail and Calendar responses have never been
parsed by ReMa's current code. Scopes, redirect handling and
refresh-token storage in the OS keychain have not been proven against
Google.

**What closes it:**

1. The owner creates or provides:
   - a Google Cloud project;
   - an OAuth consent screen, with the tester added as a test user;
   - a Desktop OAuth client;
   - the Gmail API and Google Calendar API enabled.
2. Build on the Mac with the IDs:

   ```sh
   GOOGLE_DESKTOP_CLIENT_ID=… GOOGLE_DESKTOP_CLIENT_SECRET=… \
   GOOGLE_PUBLISHING_STATUS=testing pnpm build:app
   ```

3. A person clicks Connect for Gmail and Google Calendar and completes
   Google's consent.
4. Check:
   - A chat question about recent mail uses the Gmail tool.
   - Switching Gmail off in that chat's **+** menu removes the tool
     (the next answer does not read mail).
   - After quitting and reopening ReMa, the card still shows Connected
     and a Gmail question still works.
   - Disconnect removes the keychain entry.
   - Reconnect works.
5. To verify the 7-day message, keep the account unused for more than
   7 days in Testing mode. The card must name Testing mode and say that
   the fix belongs to the Google project owner.

**Acceptance:** all of the above observed in the real app, with logs, and
no token in logs, SQLite, the frontend or model requests.

---

### B2. Live Microsoft: sign-in, Outlook Mail, Outlook Calendar

**Status:** BLOCKED. **Root causes:** RC1, RC2, RC4, RC6.

**Why it could not be done:**

1. **No Entra (Azure AD) app registration.** ReMa uses a **public client**
   (no secret; embedding one is forbidden). `MICROSOFT_PUBLIC_CLIENT_ID`
   was not available.
2. **`graph.microsoft.com` is blocked** by the network policy. Even with a
   token, no Mail or Calendar call can be made from this environment.
   `login.microsoftonline.com` is reachable.
3. A person must sign in (RC4).
4. **Tenant policy (RC6).**
   - Work and school tenants can require **admin consent** for `Mail.Read`
     and `Calendars.ReadWrite`.
   - A multi-tenant app without **publisher verification** is blocked from
     user consent in many tenants.
   - Conditional Access can refuse sign-ins.
   - None of this can be simulated.

**Scopes:** `User.Read`, `Mail.Read`, `Calendars.ReadWrite`, `offline_access`.

**What is already built:**

- Graph mail and calendar code, including delta sync and `getSchedule`.
- The same toggle, renewal and diagnostics code as Google.
- Microsoft's AADSTS codes are mapped to causes:

  | Cause | AADSTS codes |
  |---|---|
  | Inactive | 700082, 70008 |
  | Revoked | 50173, 50133, 65001 |
  | Security policy | 50076, 50079, 50078, 50072, 50158, 53003, 50055, 530003 |
  | Blocked | 50057, 50053, 50034, 50064 |

**Risk:**

- Real Graph responses are unproven.
- Personal Microsoft accounts (outlook.com) and work accounts behave
  differently and neither was tried.
- The 90-day inactivity expiry and Conditional Access prompts were only
  simulated.

**What closes it:**

1. Register an Entra app:
   - account types: "Accounts in any organizational directory and personal
     Microsoft accounts";
   - a "Mobile and desktop" platform with loopback redirect
     `http://localhost`;
   - public client flows allowed;
   - the delegated permissions above.
2. Build with `MICROSOFT_PUBLIC_CLIENT_ID=…`.
3. Sign in with one personal and one work account. For the work account,
   use a tenant where the tester can grant consent, or get admin consent.
4. Run the same checks as B1 with Outlook Mail and Outlook Calendar.
5. Run it on a network that reaches `graph.microsoft.com`. That means the
   Mac, or this environment after the network policy is changed.

---

### B3. OpenAI API: native web search, live

**Status:** BLOCKED. **Root causes:** RC1, RC2.

**Why it could not be done:** there was no OpenAI API key, and
**`api.openai.com` is blocked**. The connection is never established, so
even a key would not help in this environment.

**What is already built:**

- The Responses API path is in `src-tauri/src/llm/openai_responses.rs`:
  - the `web_search` tool;
  - `include: ["web_search_call.action.sources"]`;
  - `store: false`;
  - encrypted reasoning items replayed between rounds.
- Live test `live_openai_native_search` in `src-tauri/src/llm/live_tests.rs`.
- The "Check now" proof (P5).

**Never seen for real:**

- That a current GPT-5-family model, with `web_search` plus ReMa's
  function tools, returns `web_search_call` items and `url_citation`
  annotations in the shape the parser expects.
- That the proof judges a real response correctly.

**What closes it:** run
`REMA_LIVE_OPENAI_API_KEY=… cargo test --locked --lib live_openai -- --ignored --nocapture --test-threads 1`
on a network that reaches `api.openai.com`, and set
`REMA_LIVE_OPENAI_MODEL` to the model you mean (see L8). Also run
Settings → Career Search → **Check now** with an OpenAI model as default.
Expect `VERIFIED …` with the number of searches, pages, citations and the
page opened.

---

### B4. ChatGPT sign-in (Codex runtime): native web search, live

**Status:** BLOCKED. **Root causes:** RC1, RC2, RC4.

**Why it could not be done:**

- A ChatGPT account connects through the bundled **Codex** runtime (its
  app-server), which needs an interactive ChatGPT sign-in in a browser.
- `chatgpt.com` and `auth.openai.com` are blocked here.
- The live test does not sign in. It needs `REMA_LIVE_CODEX_HOME` pointing
  to a Codex home folder that is **already signed in**. On a Mac with ReMa
  that is
  `~/Library/Application Support/cloud.datasound.rema/runtimes/codex`.

**What is already built:**

- `src-tauri/src/accounts/codex.rs` starts Codex with `web_search="live"`
  (OpenAI runs the searches).
- The runtime's web activity is shown in the chat.
- Live test `live_codex_native_search`.

**Related limitation:** Codex fixes a thread's web search when the turn
starts. ReMa cannot switch it off mid-answer, so a Codex answer gets
either connectors or search, not both (L2).

**What closes it:** sign in to ChatGPT in ReMa on the Mac. Then run
`REMA_LIVE_CODEX_HOME=… cargo test --locked --lib live_codex -- --ignored --nocapture --test-threads 1`
and **Check now** with the ChatGPT account as default.

---

### B5. Anthropic: native search and the mail + search + thinking answer, live

**Status:** BLOCKED. **Root causes:** RC1, RC5, RC7.

**Why it could not be done:** there was no Anthropic API key.
`api.anthropic.com` is reachable (it answers 401), but the session's own
credentials must not be used (RC5).

**What is already built** (`src-tauri/src/llm/anthropic.rs` and `llm/mod.rs`):

- **Web tools** `web_search_20260318` and `web_fetch_20260318`, confirmed
  as current from the live documentation during the pass.
- **Held searches.** When Claude asks for a search and a private tool
  (e.g. Gmail) in the same turn, the search is "held". Once the private
  tool reads data, ReMa removes web tools for the rest of the answer, and:
  - it reports each held search as not run
    (`PRIVATE_WEB_NOT_RUN = "Not run: this answer read your private data first."`);
  - it strips the **unresolved** `server_tool_use` blocks from the
    history (`unresolved_web_calls`, `is_unresolved`), because the API
    returns 400 for a server tool that is no longer declared.
- **Thinking.** Newer Claude models think by default, and their thinking
  blocks are bound to the system prompt, tools and messages ("preserved
  thinking"). Removing tools therefore invalidates earlier thinking. When
  web tools were dropped (`request.web_dropped`) on a model that thinks by
  default (`thinks_by_default`), ReMa sends:
  - `thinking: {type: "adaptive", block_binding: {prefix_mismatch_behavior: "drop_block"}}`
  - with beta `thinking-binding-controls-2026-08-01` (`BINDING_BETA`).
- **One beta header.** `authorize()` merges the Claude sign-in (OAuth)
  beta and this beta into one `anthropic-beta` header.
- **HTTP-level test:**
  `a_search_held_next_to_a_private_tool_is_dropped_with_the_web`
  (`src-tauri/src/llm/tests_http.rs`).

**Open questions that only a live request can answer (RC7):**

a. Does the API accept `block_binding.prefix_mismatch_behavior: "drop_block"`
   with that beta for the models ReMa uses (Opus 5.5, Fable 5.1, Sonnet 5)?
   Does it also accept it with a **Claude sign-in (OAuth) token**, where the
   OAuth beta and this beta share one header?
b. Is explicit `thinking: {type: "adaptive"}` the same as the model's
   default, or does it change cost or behaviour?
c. After the web is dropped, the history still holds **completed**
   `server_tool_use` and `web_search_tool_result` blocks, while the request
   no longer declares the web tools. Only unresolved blocks are stripped.
   Does the API accept completed ones? If not, every chat that searched
   and then read mail fails at the next round.
d. Is an assistant turn from which an unresolved `server_tool_use` was
   removed, followed by the user's `tool_result` for Gmail, accepted?
e. On accounts where preserved thinking is **enforced** (accounts created
   on or after 2026-08-31, on Opus 5.5 and Fable 5.1), does `drop_block`
   avoid the 400 as documented?
f. **There is no fallback.** If a gateway or account rejects the beta
   header, the answer fails. ReMa does not retry without it.

**Alternative not tried:** beta `mid-conversation-tool-changes-2026-07-01`
with `tool_removal`. It may keep earlier reasoning where `drop_block`
discards it (L5). It is worth evaluating against the current Anthropic
documentation.

**Risk:** if (a), (c) or (d) is wrong, on Claude every chat that reads
mail or calendar while web search is on fails with HTTP 400 in its second
round. This is the most important item to verify.

**What closes it:** run
`REMA_LIVE_ANTHROPIC_API_KEY=… cargo test --locked --lib live_anthropic -- --ignored --nocapture --test-threads 1`.
This runs:

- `live_anthropic_native_search`;
- `live_anthropic_mail_and_search_in_one_answer`, a stand-in mailbox
  holding canary `ZEBRA-HARBOUR-7731` plus real Claude.

Repeat with a Claude sign-in (OAuth) instead of a key, in the app. If
anything returns 400, capture the request body and the error, and use
Anthropic's preserved-thinking guidance, with its diagnosis header where
present, to find which block broke the prefix.

---

### B6. Gemini 3: native search, live

**Status:** BLOCKED. **Root causes:** RC1, RC7.

**Why it could not be done:** there was no Gemini API key. The host is
reachable.

**What is already built** (`src-tauri/src/llm/gemini.rs`):

- The `google_search` tool, with `functionDeclarations` next to it only
  on models that allow both (`combines_search_with_functions`, Gemini 3
  and later).
- `includeServerSideToolInvocations: true` when both are sent.
- Grounding metadata parsed into sources.
- Live test `live_gemini_native_search`.

**Never seen for real:**

- The combined search-plus-functions mode on a real Gemini 3 model.
- The shape of grounding metadata.
- Whether the model auto-pick finds a model with `gemini-3` in its id.

**What closes it:**
`REMA_LIVE_GEMINI_API_KEY=… cargo test --locked --lib live_gemini -- --ignored --nocapture --test-threads 1`,
plus **Check now** with Gemini as default.

---

### B7. Packaged app on macOS (and on Windows)

**Status:** BLOCKED. **Root cause:** RC3.

**Why it could not be done:** the environment is Linux only.
`.github/workflows/release.yml` has a `macos-14` job that could at least
build the app, but it was not run:

- `build.rs` fails without the connector Actions variables.
- An agent cannot open a GUI app on that runner from Finder or the Dock.

**Never checked on macOS:**

1. **Launching from Finder, the Dock or a login item.** launchd starts the
   app with a minimal environment. On Linux this was reproduced with
   `env -i` and it worked (P9), but macOS was not tried.
2. **The login shell.**
   - `SHELL` may be missing. ReMa then reads the shell from
     `dscl . -read /Users/<user> UserShell`, and falls back to
     `/bin/zsh` on macOS (`src-tauri/src/accounts/locate.rs`).
   - zsh is asked with `-i -l -c`. Real `.zshrc` files may print text,
     wait for input or be slow (oh-my-zsh, Powerlevel10k instant prompt,
     conda init). Output parsing and timeouts are untested there.
3. **MCP servers started with `npx`, `uvx`, nvm, Homebrew, asdf, mise or
   volta** from the packaged app on macOS.
4. **The macOS system proxy.** ReMa's own requests use reqwest's
   `system-proxy`, which reads the macOS proxy settings. MCP child
   processes get proxy variables only if the login shell sets them.
5. **Keychain prompts.**
   - Local builds are ad-hoc signed, so the signature changes with every
     rebuild.
   - macOS may then ask again for Keychain access to ReMa's stored
     credentials, or deny it while ReMa runs in the background.
   - A stable Developer ID signature is needed.
6. **Gatekeeper and notarization.** `tauri.conf.json` has no signing
   identity and the release workflow does not notarize. A downloaded build
   is blocked by Gatekeeper; local builds are unaffected.
7. **Bundling on macOS.** `pnpm build:app` downloads and checksums the
   Codex and ant runtimes. Architecture (arm64 vs x86_64) and bundling
   were not tried on macOS.
8. **macOS behaviour:** autostart as a login item, close-to-background,
   tray, and notifications.

**Windows:** the release workflow builds for Windows, but nothing was run
there. The login-shell lookup in `locate.rs` is written for Unix shells.

**What closes it:**

1. On a Mac, build with `pnpm build:app` and install to `/Applications`.
2. Start it from Finder, from the Dock and as a login item after a reboot.
3. In each case, add MCP servers that use nvm's `node` (`npx`) and `uvx`.
   Check in ReMa's MCP settings that they connect, and which `node` they
   used. The env-probe server used on Linux is a good template: it
   reports `PATH`, `node`, the proxy and the variables it saw.
4. For signing, the owner needs an Apple Developer ID, plus notarization
   in `release.yml`.

---

### B8. Other apps calling ReMa MCP with a real model

**Status:** BLOCKED (the call); PARTIAL (setup, see P6–P8).
**Root causes:** RC1, RC3, RC4, RC5.

**Why it could not be done:**

| App | What was done | What blocked the rest |
|---|---|---|
| Claude Code 2.1.283 | `claude mcp add --scope user rema -- /usr/bin/rema mcp`, then `claude mcp list` showed ✔ Connected | A tool call needs Claude Code to run a model. That needs a login or API key, and session credentials are forbidden. |
| Codex CLI 0.157.1 | `codex mcp add` wrote exactly ReMa's TOML snippet; `mcp list` showed it enabled | A call needs a ChatGPT sign-in or an OpenAI key, and `api.openai.com` is blocked. |
| Claude Desktop | ReMa's config writer and backup tested; ReMa imported four servers from a Claude Desktop file | Claude Desktop has no Linux build. |
| Cursor, VS Code, Windsurf | Config writer and backup tested | GUI apps with their own model accounts. |
| MCP Inspector (official TypeScript SDK) | **VERIFIED**: 5 tools listed, `source_status` returns a result that passes output-schema validation, `--strict` clean | — |

**Risk:** `search_jobs` was never seen returning jobs through MCP,
because the job boards are blocked (B9). A model in another app has never
chosen and called ReMa's tools.

**What closes it:** on the Mac, in each app, add ReMa from ReMa's
Settings (the **Use ReMa MCP in other apps** panel: "Add to Claude
Desktop / Cursor / VS Code", or the Claude Code and Codex snippets). Then ask the model:

- "Use the rema tool source_status."
- "Use rema to search jobs for a Rust developer in Berlin."

Record the tool call and result shown in each app.

---

### B9. Live job search through ReMa's sources and ReMa MCP

**Status:** BLOCKED. **Root cause:** RC2.

**Why it could not be done:** every job source ReMa reads is blocked by
the network policy (`www.arbeitnow.com`, `boards-api.greenhouse.io`,
`api.lever.co`, `remotive.com` and others). Earlier in the pass arbeitnow
answered `connect_rejected` and later **403**. Whether that 403 came from
the proxy or from the site's bot protection is unknown.

**Never seen for real:**

- The source adapters parsing today's live responses.
- Ranking and de-duplication on real data.
- Verified search mode opening real postings (P11).
- `search_jobs` over MCP returning jobs.
- Behaviour under real rate limits and bot blocks.

**What closes it:** run on the Mac, or allow those hosts in the
environment's network settings. Then:

- Ask a chat for jobs.
- Run the Career Search check.
- Call `search_jobs` from the MCP Inspector:
  `npx @modelcontextprotocol/inspector --cli /Applications/ReMa.app/Contents/MacOS/rema mcp --method tools/call --tool-name search_jobs …`.

Every returned posting must open, and its title and company must match the
page.

---

## 3. Partials

### P1. Per-chat connector toggles across restart, reauth, reconnect and disconnect

**Status:** PARTIALLY VERIFIED.

**Done:** test `each_chat_chooses_its_connectors_across_restart_reauth_disconnect_and_reconnect`
(`src-tauri/src/services/connector_tools/tests.rs`). It reopens a
file-based database and credential store to act as a restart, and checks:

- toggles change only a chat's tools, never the connection;
- cards stay connected;
- a refused renewal marks only that account;
- disconnect and reconnect behave.

**Gap:** the provider side is a stand-in (B1, B2). Never seen for real:

- A real Google or Microsoft refresh after a restart, from the OS keychain.
- Revocation at the provider on disconnect.
- A real refused renewal.

**Closes with:** B1 and B2.

### P2. Renewal never reads mail or calendars

**Status:** PARTIALLY VERIFIED.

**Done:** test `connected_accounts_are_kept_signed_in_without_reading_anything`
checks that only the token endpoint was called, against stand-in endpoints.

**Gap:** it has not been proven against the real providers.

**Closes with:** a real run with a logging proxy (for example mitmproxy,
with its CA trusted by ReMa, never by disabling TLS checks) while ReMa
runs for an hour with connected accounts and no chat. Only
`oauth2.googleapis.com/token` and
`login.microsoftonline.com/.../token` may appear, and no Gmail or Graph
data URL. Alternatively, check the provider's account activity.

### P3. Reasons shown when a connection ends

**Status:** PARTIALLY VERIFIED.

**Done:**

- `connectors/failure.rs`: `reauth_cause` and `reauth_message`, stored
  through migration `0016`.
- Tests `a_refused_renewal_is_traced_to_its_cause`,
  `a_reconnect_says_why_google_ended_the_connection_and_whose_setting_it_is`,
  `a_testing_google_app_says_when_the_sign_in_ends` and
  `an_unused_microsoft_connection_says_it_expired_after_90_days`.

**Gaps (why this is partial):**

1. **Built from documentation, not real errors.** Google and Microsoft
   error texts and AADSTS codes were mapped from their documentation.
   No real error response was captured. A different real wording could
   fall through to a generic cause.
2. **Testing-mode inference is a heuristic.** Google returns the same
   `invalid_grant` ("Token has been expired or revoked") for all of these:
   - Testing expiry;
   - the user revoking access;
   - a password change on an account with Gmail scopes;
   - the 100-refresh-tokens-per-client limit;
   - 6 months unused.

   When the build does not say its publishing status
   (`GOOGLE_PUBLISHING_STATUS` empty), ReMa blames Testing mode if the
   connection is at least 7 days old, minus 10 minutes of slack. A user
   who revokes access on day 8 is therefore told it was Testing expiry.
3. **The age is measured from `connected_at`,** which ReMa stores itself
   and resets on every connect or reconnect. That is right for a fresh
   grant, but it is ReMa's clock, not the time Google issued the token.
4. **Not told apart:** Google's 100-token limit and 6-month inactivity.

**Closes with:** capture real error bodies during B1 and B2 (revoke in
Google Account → Security, or in Microsoft My Apps; wait out Testing
expiry) and adjust the mapping. For releases, set
`GOOGLE_PUBLISHING_STATUS` so the heuristic is not needed.

### P4. Anthropic mixed flow

**Status:** PARTIALLY VERIFIED, at the request-shape level only.

**Done:** an HTTP test with a stand-in Anthropic server shows that:

- the held search is left out and reported as not run;
- `drop_block` and its beta are sent;
- the mail call and its result are kept;
- no web tools are sent after the read.

**Gap:** everything listed under B5 (a–f).

**Closes with:** B5.

### P5. Native search proof ("Check now")

**Status:** PARTIALLY VERIFIED (unit tests only).

**Done:**

- `src-tauri/src/career_search/proof.rs`: `prove()`, `SearchProof`,
  `verdict()`.
- `career_search/status.rs` runs it with a 120-second timeout.
- The verdict counts only the provider's own search events, the pages it
  returned, citations, and one page ReMa opened itself through its safe
  fetcher.
- Test `only_what_the_provider_reported_counts`.

**Gaps:**

1. It never ran against a real provider (B3–B6).
2. It **forces** a search (`WebSearch{required: true}`). That proves the
   provider can search, but not that the model decides to search on an
   ordinary question the way ChatGPT and Claude do.
3. The question asks for the newest stable Rust release on
   `blog.rust-lang.org`. That host is blocked here, so the "opened a page"
   step would fail here even with a key.
4. A page that answers **403** (bot protection) counts as opened (it
   exists).
5. Citations are reported but not required.
6. The answer text is not checked for correctness.

**Closes with:**

- A live run per provider.
- An optional unforced run with a question that clearly needs the web,
  counting how often the model searches without being forced.

### P6. ReMa MCP in Claude Code

**Status:** PARTIALLY VERIFIED. Claude Code added ReMa and its health
check says ✔ Connected. No model called a tool. See B8.

### P7. ReMa MCP in Codex

**Status:** PARTIALLY VERIFIED. Codex wrote the config ReMa proposes and
lists the server as enabled. No call was made. See B8.

### P8. ReMa MCP in Claude Desktop, Cursor, VS Code and Windsurf

**Status:** PARTIALLY VERIFIED.

**Done:** ReMa writes into each app's config file without removing the
user's other servers, and keeps a backup (test
`adds_itself_to_another_apps_settings_and_keeps_its_servers`, plus the X7
run).

**Gap:** the apps themselves were never run. See B7 and B8.

### P9. MCP servers' environment in a packaged app

**Status:** VERIFIED on Linux, BLOCKED on macOS.

**Done:** the installed release `.deb` was started with `env -i`
(no `SHELL`, `PATH=/usr/bin:/bin`), as launchd or a desktop launcher does.
The login profile set nvm, uv, a proxy with its CA, and a token. Results:

- `npx -y @modelcontextprotocol/server-everything` connected (13 tools).
- `uvx mcp-server-time` connected (2 tools).
- A probe server saw:
  - the token;
  - nvm's `node`;
  - the proxy;
  - no `REMA_*` variables;
  - its own configured value winning over the shell's.
- All of them reconnected after a restart.

**Gap:** macOS (B7).

### P10. Older HTTP+SSE MCP servers

**Status:** VERIFIED for the basic case: the official Python SDK server
connected and a tool call worked. Gaps remain in the design.

**Done:** `src-tauri/src/mcp/legacy_sse.rs` follows the MCP
specification's backwards-compatibility procedure:

1. Try Streamable HTTP first.
2. If that fails, `GET` the event stream and wait for its `endpoint` event
   (up to 20 s, `ENDPOINT_WAIT`).
3. `POST` messages only to that address, and only on the same origin.

rmcp 0.11 removed its own SSE transport, so ReMa implements rmcp's
`Transport` itself. Importing a config with `"type": "sse"` maps to this
path.

**Gaps:**

1. **No OAuth for SSE servers.** The fallback runs only when the
   Streamable HTTP error is **not** 401/403 (`is_unauthorized` in
   `mcp/client.rs`). A legacy SSE server that needs MCP OAuth is shown as
   needing sign-in and never reaches the SSE path. Static headers from the
   config (API key, `Authorization`) are sent.
2. **No reconnect or resume.** If the `GET` stream drops, the session ends
   and the server must be reconnected as a whole.
3. **Messages before `endpoint`.** Messages that arrive before the
   `endpoint` event go into a 64-slot channel with `try_send`. On overflow
   they are **dropped silently** (`let _ = tx.try_send(message)`).
4. **The fallback runs on any non-auth error** (timeouts, 5xx, DNS). A
   server that is simply down therefore costs one more request, and up to
   20 s more if it answers the `GET` without sending `endpoint`. The
   original error is kept for the message.
5. **Another origin is refused.** An `endpoint` on another origin is
   refused on purpose, for security. A server that legitimately uses one
   will not work.
6. **WebSocket is not supported.** It is not an MCP standard transport.
   Import refuses `ws` and `websocket` entries with a clear message
   instead of faking support.

**Closes with:** decide whether 1–4 matter.

- For 3: switch to an awaited `send`, or log dropped messages.
- For 2: reconnect on stream end.
- For 1: this needs MCP OAuth (discovery plus PKCE) on the SSE path,
  reusing the Streamable HTTP OAuth code.

### P11. ReMa's verified search mode

**Status:** PARTIALLY VERIFIED.

**Done:** the orchestrator, validation and answer tests with stand-in
sources (`searches_validates_then_answers_about_what_it_found`, and the
runs in `validation.md`).

**Gap:** real sources (B9).

### P12. OpenAI API: web search switched off mid-answer

**Status:** PARTIALLY VERIFIED (unit and HTTP tests only).

**Done:** with an API key, OpenAI takes the tool list anew each round
(`web_switches_off_between_rounds` is true for API keys). When private data
is read, the next request drops `web_search`. Earlier rounds' encrypted
reasoning items are replayed (`store: false`).

**Gap:** it is untested live whether OpenAI accepts replayed reasoning and
`web_search_call` history when `web_search` is no longer declared, and
whether reasoning quality holds. This is the OpenAI counterpart of B5 (c).

**Closes with:** a live run once `api.openai.com` is reachable. A useful
live test would mirror `live_anthropic_mail_and_search_in_one_answer`.

---

## 4. Known bugs and limitations left as they are

| ID | Issue | Where | Why it was left |
|---|---|---|---|
| L1 | **Settings shows empty sections for several seconds with no loading indicator.** In the packaged app, the provider and connector sections were blank before content appeared. The likely cause is that first-load commands wait on the keychain or network, but it was not investigated. | `src/pages/SettingsPage.tsx`, `src/components/settings/ConnectorsSection.tsx` (no loading state), `ProviderRows.tsx` | Out of scope for the pass; seen during the packaged run. A real UI bug; fix it with loading placeholders and by finding which command is slow. |
| L2 | **Codex (ChatGPT sign-in) and Gemini answers get connectors or search, never both.** Their search cannot be switched off between rounds. A question about the user's own data gets connectors; any other question gets search. ChatGPT and Claude can do both in one answer. | `src-tauri/src/services/chat.rs` (`web_switches_off_between_rounds`, the `exclusive` rule) | Needed for the privacy rule (no web after private data). Changing it needs a provider feature to switch search off mid-turn. |
| L3 | **Once an answer reads private data, web search and ReMa MCP are off for the rest of that chat.** This is stricter than Claude.ai; the user must start a new chat to search again. | `llm/mod.rs` (private drop), `services/chat.rs` (`private_history`), `services/chat_tools.rs` | Deliberate: mail text is untrusted and could exfiltrate data through search queries. Any relaxation needs a security review. |
| L4 | **In a chat whose connector toggles were changed, a connector connected later starts OFF.** The chat then has its own list, and new connectors are not added to it. | connector toggles (`conversation.connectors`) | Deliberate privacy default. Could be made a setting. |
| L5 | **`drop_block` discards Claude's earlier reasoning when web tools are removed,** which may lower answer quality. | `llm/anthropic.rs` | Only known way to keep the request valid. The `tool_removal` beta (B5) may do better. |
| L6 | **The MCP Inspector warns "unknown format uint32"** for integer formats that `schemars` emits in ReMa MCP schemas. | `src-tauri/src/rema_mcp/server.rs` (`portable`) | Warnings only; validation passes. Strip non-standard `format` values in `portable()` to silence them. |
| L7 | **Schema portability was checked only for ReMa MCP's 5 tools.** ReMa's internal tool schemas sent to models (connector, network and business tools) were not checked against single-type dialects or OpenAI strict mode. | `services/connector_tools`, `network`, business tools | Not in scope. Apply the `tool_schemas_use_one_type_per_value` test to them. |
| L8 | **Live tests pick a model automatically**: the first listed id containing `opus`, `gpt-5` or `gemini-3`, else the first model. They may pick an old, preview or expensive model. | `src-tauri/src/llm/live_tests.rs` (`pick_model`) | Set `REMA_LIVE_<PROVIDER>_MODEL` explicitly. |
| L9 | **The user's Mac install is a debug build without OAuth IDs.** Google and Outlook sign-in are unavailable. Developer `REMA_*` overrides are active. It is slower. `GOOGLE_PUBLISHING_STATUS` is unset. | user's Mac | The user chose a build without IDs for now. Rebuild with IDs and `pnpm build:app` (release) to test B1 and B2. |
| L10 | **LinkedIn and XING: only imported exports.** LinkedIn's connections API (`r_1st_connections`) needs partner approval, and XING has no public API. Sign-in with LinkedIn gives only name and e-mail (`openid profile email`). | `src-tauri/src/connectors/linkedin.rs`, `network/contacts.rs` | Policy (RC6). Cannot be solved by code; do not fake live access. |
| L11 | **Windows was never run.** | release matrix | RC3. |
| L12 | **The release workflow cannot build without the Actions variables and does not sign or notarize** macOS or Windows builds. | `.github/workflows/release.yml`, `src-tauri/tauri.conf.json` | Needs the owner's IDs and signing certificates. |

---

## 5. Rules the next agent must keep

These come from the user and are not negotiable. A "fix" that breaks one
of them is not a fix.

- **Credentials.**
  - Never send OAuth tokens, refresh tokens, PKCE data or keys to a model.
  - Never store credentials in localStorage, sessionStorage, frontend
    state, plaintext SQLite, JSON config, logs, analytics or prompt
    history. The OS keychain only.
  - Never embed a Microsoft client secret; ReMa is a public client.
- **Sign-in and connections.**
  - Never automate an interactive OAuth sign-in.
  - Never open OAuth windows repeatedly on your own.
  - Never show Connected when it is not.
  - Refreshing a token must never read mail or calendar data.
  - Connector toggles control a chat's tools, not the sign-in.
- **Untrusted content.**
  - E-mail bodies are untrusted.
  - No web search after private data in the same answer, enforced in code
    (tool availability), not only in prompts.
- **Network.**
  - Never disable TLS verification.
  - Never unset `HTTPS_PROXY`.
  - Never use the session's infrastructure credentials (`CLAUDE_CODE_*`,
    `GH_TOKEN` and similar) for tests or model calls.
- **Other apps' configuration.**
  - Never overwrite it destructively; back it up.
  - Do not overwrite MCP environment variables set in a server's config
    with inherited shell values.
- **Claims.**
  - Do not fake support for a transport or an API.
  - Do not claim Google persistence is solved while the OAuth project is
    in Testing.
  - Do not present imported contacts as live LinkedIn or XING access.
  - VERIFIED requires real execution evidence. Mocks never count.
- **Git and process.**
  - Work only on `claude/rema-foundation-setup-6gmim3`.
  - No pull request unless asked.
  - No model identifiers in commits or code.
  - End-to-end runs use a scratch `HOME`.

---

## 6. What a human must provide

| Needed for | Input | How it reaches the test or app |
|---|---|---|
| B5, P4 | Anthropic API key (and a Claude sign-in for the OAuth check) | `REMA_LIVE_ANTHROPIC_API_KEY`, or an environment secret |
| B3, P12 | OpenAI API key **and** a network that reaches `api.openai.com` | `REMA_LIVE_OPENAI_API_KEY` |
| B6 | Gemini API key | `REMA_LIVE_GEMINI_API_KEY` |
| B4 | A Codex home signed in with ChatGPT | `REMA_LIVE_CODEX_HOME` |
| B1, P1–P3 | Google Cloud project, Desktop OAuth client, test user, APIs enabled; a person to sign in | `GOOGLE_DESKTOP_CLIENT_ID`, `GOOGLE_DESKTOP_CLIENT_SECRET`, `GOOGLE_PUBLISHING_STATUS` at build time |
| B2, P1–P3 | Entra public client, personal and work test accounts, possibly admin consent; a network that reaches `graph.microsoft.com` | `MICROSOFT_PUBLIC_CLIENT_ID` at build time |
| B7, P9, B8 | A Mac (ideally also a Windows PC); a Developer ID and notarization for distribution | — |
| B9, P11, B8 | A network that reaches the job boards | environment network settings, or run on the Mac |
| Production Google | Google OAuth verification and security assessment for `gmail.readonly` | owner task (RC6) |

If this cloud environment is used again, the owner can add the keys as
environment secrets. They can also allow `api.openai.com`,
`graph.microsoft.com`, `chatgpt.com`, `auth.openai.com` and the job-board
hosts in the environment's network settings. Interactive sign-ins (B1,
B2, B4) still need a person at a browser.

## 7. Commands

All live tests at once, on the Mac from the repository root's `src-tauri/`
folder:

```sh
REMA_LIVE_ANTHROPIC_API_KEY=… REMA_LIVE_OPENAI_API_KEY=… REMA_LIVE_GEMINI_API_KEY=… \
REMA_LIVE_CODEX_HOME="$HOME/Library/Application Support/cloud.datasound.rema/runtimes/codex" \
  cargo test --locked --lib live_ -- --ignored --nocapture --test-threads 1
```

- Each test prints `VERIFIED …` with its evidence, `FAILED …` with the
  reason, or `BLOCKED …` naming the missing variable.
- Optional model choice: `REMA_LIVE_ANTHROPIC_MODEL`,
  `REMA_LIVE_OPENAI_MODEL`, `REMA_LIVE_GEMINI_MODEL`,
  `REMA_LIVE_CODEX_MODEL`.
- Legacy SSE against a real server:
  `REMA_LIVE_SSE_URL=http://host/sse cargo test --locked --lib live_legacy_sse_server -- --ignored --nocapture`.

Regular checks before any push:

```sh
cd src-tauri && cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked
cd .. && pnpm lint && pnpm typecheck && pnpm test
```

## 8. File map

| Area | Files |
|---|---|
| Anthropic request, web tools, thinking, betas | `src-tauri/src/llm/anthropic.rs` |
| Tool loop, private-data drop | `src-tauri/src/llm/mod.rs` |
| OpenAI Responses API | `src-tauri/src/llm/openai_responses.rs`, `src-tauri/src/llm/openai.rs` |
| Gemini | `src-tauri/src/llm/gemini.rs` |
| Codex runtime | `src-tauri/src/accounts/codex.rs` |
| Live tests | `src-tauri/src/llm/live_tests.rs` |
| HTTP-level provider tests | `src-tauri/src/llm/tests_http.rs` |
| Search proof and Career Search check | `src-tauri/src/career_search/proof.rs`, `src-tauri/src/career_search/status.rs` |
| Which tools an answer gets | `src-tauri/src/services/chat.rs`, `src-tauri/src/services/chat_tools.rs` |
| Connectors, toggles, tokens, causes | `src-tauri/src/connectors/{mod,tokens,failure,config,build_config}.rs`, `src-tauri/src/db/connectors.rs`, `src-tauri/src/db/migrations/0016_reauth_cause.sql`, `src-tauri/connectors.toml` |
| Connector tool tests | `src-tauri/src/services/connector_tools/tests.rs`, `src-tauri/src/connectors/tests.rs` |
| MCP client, shell lookup, HTTP+SSE, import | `src-tauri/src/mcp/client.rs`, `src-tauri/src/accounts/locate.rs`, `src-tauri/src/mcp/legacy_sse.rs`, `src-tauri/src/mcp/import.rs` |
| ReMa MCP server | `src-tauri/src/rema_mcp/server.rs` |
| Imported contacts | `src-tauri/src/network/contacts.rs`, `src-tauri/src/connectors/linkedin.rs` |
| Settings UI | `src/pages/SettingsPage.tsx`, `src/components/settings/*`, `src/lib/connectorNotes.ts` |
| Packaging | `.github/workflows/release.yml`, `src-tauri/tauri.conf.json`, `src-tauri/build.rs` |
| Evidence so far | `docs/parity/verification.md`, `docs/parity/implementation.md`, `docs/parity/validation.md` |

## 9. Suggested order of work

1. **B5 first.** A wrong guess there breaks every Claude chat that reads
   mail with search on. It needs only an Anthropic key.
2. **B3, B6, B4 and P12:** the other providers' search, and OpenAI's
   mid-answer drop.
3. **B7 on a Mac:** a packaged launch from Finder, Dock and a login item,
   with nvm and uvx MCP servers. Then B8 in Claude Code, Claude Desktop
   and Codex.
4. **B1 and B2** with real OAuth clients and accounts. Capture real error
   bodies to finish P3; check P2 with a logging proxy.
5. **B9 and P11** on a network that reaches the job boards.
6. **L1** (Settings loading) and **P10** items 2–3 (SSE reconnect and
   dropped messages): small, safe code fixes.
7. **Owner decisions:**
   - Google publication and verification;
   - Apple and Windows signing;
   - whether to relax L2, L3 or L4, after a security review.
