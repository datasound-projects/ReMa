# Connectors, search and MCP: verification (second pass)

Follows [implementation.md](implementation.md) and [validation.md](validation.md).
Each area is classified by the evidence behind it:

- **VERIFIED**: executed for real, against the real counterpart.
- **PARTIALLY VERIFIED**: ReMa's own code executed end to end, but a
  provider or platform was stood in for, or only part was reachable.
- **BLOCKED**: needs something this environment does not have; what is
  needed is listed.
- **FAILED**: executed and did not work (none left open; failures found
  during this pass are listed with their fixes).

Environment: Linux container, Xvfb, D-Bus with GNOME Keyring. No provider
API keys, no OAuth app registrations or test accounts; the network policy
blocks `api.openai.com`, `graph.microsoft.com` and the job boards ReMa's
sources read. Session credentials were never used.

## Checks run

| Check | Result |
|---|---|
| `cargo fmt --check`, `cargo clippy --all-targets --locked -- -D warnings` | clean |
| `cargo test --locked` | 789 passed, 9 ignored (opt-in live tests) |
| `pnpm lint`, `pnpm typecheck`, `pnpm test` | clean; 141 passed |
| Production build (`pnpm tauri build --bundles deb`, release profile, format-valid placeholder OAuth IDs, `GOOGLE_PUBLISHING_STATUS=testing`) | built in 11m43s; `ReMa_0.1.0_amd64.deb` 15.7 MiB; installed with `dpkg -i` |
| Live tests (`cargo test --lib live_ -- --ignored`) without keys | each prints `BLOCKED …` with the variable it needs |

## Results

| # | Area | Status | Evidence |
|---|---|---|---|
| 1 | Per-chat connector toggles for Gmail, Google Calendar, Outlook Mail, Outlook Calendar: new and existing chats, restart, refused renewal, reconnect, disconnect | PARTIALLY VERIFIED | `each_chat_chooses_its_connectors_across_restart_reauth_disconnect_and_reconnect`: a file database and credential store reopened as a restart; toggles only change a chat's tools; cards stay connected. Provider side stood in for. |
| 1 | Renewal never reads mail or calendars | PARTIALLY VERIFIED | `connected_accounts_are_kept_signed_in_without_reading_anything` against stand-in endpoints. |
| 1 | Diagnostics for expired, revoked and Testing-mode sign-ins | PARTIALLY VERIFIED | `a_reconnect_says_why_google_ended_the_connection_and_whose_setting_it_is`, `a_testing_google_app_says_when_the_sign_in_ends`, `an_unused_microsoft_connection_says_it_expired_after_90_days`, `a_refused_renewal_is_traced_to_its_cause`; the card and the notification name the cause and whose setting it is. |
| 2 | Live Google OAuth, Gmail, Google Calendar, Microsoft OAuth, Outlook Mail and Calendar | BLOCKED | Needs ReMa's Google Desktop client and Microsoft public client IDs, test accounts, a browser sign-in and network access to Microsoft Graph. |
| 3 | Claude answer with a search held next to a mail tool, thinking bound to the tool list | PARTIALLY VERIFIED | HTTP-level `a_search_held_next_to_a_private_tool_is_dropped_with_the_web`: held search left out and shown as not run, `drop_block` with its beta header, mail call and result kept, no web tools after the read. Live run BLOCKED: `live_anthropic_mail_and_search_in_one_answer` needs `REMA_LIVE_ANTHROPIC_API_KEY`. |
| 4 | Native search: OpenAI API, ChatGPT via Codex, Anthropic, Gemini 3 | BLOCKED | Proof built: Settings → Career Search → Check now, and `live_*_native_search`, judge a real request only by the provider's search events, the pages returned, citations and one page opened (`career_search::proof`). Needs the keys, a signed-in Codex home, and `api.openai.com` reachable. |
| 4 | ReMa verified search still opens and checks postings | PARTIALLY VERIFIED | `searches_validates_then_answers_about_what_it_found` and the verified-mode runs in validation.md, with stand-in sources. |
| 5 | Nothing private reaches a search | VERIFIED (ReMa's orchestration) | `nothing_private_reaches_a_search` plants mail text quoted in an answer, a vCard contact, a LinkedIn export contact, an OAuth token, an MCP secret and a provider key; checks every model request that can search, in both answer modes, company research and Network Connect, plus every request to ReMa's sources. It found ReMa MCP (which searches the web) still offered, behind an approval, in chats that had read private data; now off there and refused mid-answer (`rema_mcp_is_off_once_an_answer_read_private_data`). |
| 6 | Imported contacts: local, no e-mail, never to a model or Business, labelled, no duplicates | VERIFIED | Real LinkedIn export files (CSV and ZIP) and vCards imported; the stored rows hold no `@`; the canary test above covers models; `the_same_export_imported_twice_or_twice_at_once_is_kept_once`; labels in validation.md. |
| 7 | MCP servers from a packaged app started without a shell environment | VERIFIED on Linux, BLOCKED on macOS | The installed release app started as a launcher or launchd does (`env -i`: no `SHELL`, proxy or token; `PATH=/usr/bin:/bin`). Its login profile set nvm, uv, a proxy with its CA, and a token. Results: `npx -y @modelcontextprotocol/server-everything` connected (13 tools, downloaded through the profile's proxy); `uvx mcp-server-time` connected (2 tools, from PyPI); a probe server confirmed the token, nvm on PATH, nvm's `node`, the proxy, no `REMA_*`, and its configured `OVERRIDE_ME` over the shell's value. All reconnected after a restart. Finder, Dock and login items need a Mac. |
| 7 | Timeouts | VERIFIED (code) | `CONNECT_TIMEOUT` 90 s, `CALL_TIMEOUT` 10 min. |
| 8 | ReMa MCP in Claude Code | PARTIALLY VERIFIED | Claude Code 2.1.283 in an isolated home with no credentials: ReMa's exact command `claude mcp add --scope user rema -- /usr/bin/rema mcp`, then `claude mcp list` → `rema: … ✔ Connected`. A tool call by Claude Code needs a model: BLOCKED. |
| 8 | ReMa MCP in the MCP Inspector (official TypeScript SDK client) | VERIFIED | `tools/list` returns 5 tools; `source_status` returns a structured result that passes the SDK's output-schema check; `--strict` reports no schema warnings. `search_jobs` returns its error as text (job boards blocked by the network policy). |
| 8 | ReMa MCP in Codex | PARTIALLY VERIFIED | Codex CLI 0.157.1 `mcp add rema -- /usr/bin/rema mcp` writes exactly ReMa's snippet (`[mcp_servers.rema]`, `command`, `args = ["mcp"]`); `mcp list` shows it enabled. A call needs a ChatGPT sign-in: BLOCKED. |
| 8 | Claude Desktop, Cursor, VS Code, Windsurf | PARTIALLY VERIFIED | The apps do not run here. Their files are written and backed up as tested (`adds_itself_to_another_apps_settings_and_keeps_its_servers`, and the X7 run); ReMa imported four servers from a Claude Desktop file in the packaged app. |
| 8 | Import with `${VAR}` and keychain values | VERIFIED | X7 run (keychain entry holds the expanded token) and `adds_servers_from_another_apps_configuration`. |
| 9 | Servers on the older HTTP+SSE transport | VERIFIED | The official MCP Python SDK 2.2.0 server with `transport="sse"`: the packaged app imported it from the Claude Desktop file and connected (1 tool); its log shows `POST /sse` 405, `GET /sse` 200, then `POST /messages/?session_id=…` 202. `live_legacy_sse_server` called its tool (`echo: from ReMa`). |
| 10 | Packaged app on macOS | BLOCKED | Needs a Mac (build with `pnpm build:app`, run from Finder, Dock and a login item). |

## Found and fixed in this pass

| Finding | Fix | Test |
|---|---|---|
| A refused renewal showed only "revoked or has expired". | Cause stored with the account (migration 0016) and said on the card and in the notification, with whose setting it is; `GOOGLE_PUBLISHING_STATUS` lets a build say its Google app is in Testing, and its Google cards then say when Google ends the sign-in. | see row 1 |
| The model-search check only said a model had a search. | It now runs one real search and judges the provider's evidence. | `only_what_the_provider_reported_counts` |
| ReMa MCP offered in chats that had read private data. | Off there; refused once an answer reads private data. | row 5 |
| MCP programs were looked up in ReMa's own `PATH` and install folders before the login shell's: a Homebrew or system `node` won over nvm's. | Looked up where the user's shell looks, login `PATH` first. | `a_program_is_found_where_the_users_shell_finds_it`, packaged run |
| An app started without `SHELL` (launchd, some launchers) got no login environment; csh/tcsh were asked in the wrong syntax. | The account record's shell is used (`dscl` on macOS, `getent` elsewhere); csh/tcsh are asked with `-i -c`. | `finds_the_users_shell_without_a_shell_variable`, `each_shell_is_asked_in_its_own_syntax` |
| Servers on HTTP+SSE could not be used (rmcp removed the transport in 0.11). | The MCP specification's backwards-compatible client: Streamable HTTP first, then the event stream, messages only to the server's own origin. | `a_server_on_the_older_http_sse_transport_connects_lists_and_calls`, `a_stream_naming_another_site_is_refused`, row 9 |
| ReMa MCP's schemas used `"type": [T, "null"]` (113 Inspector warnings); clients with a single-type dialect can reject the tools. | Optional inputs take one type; nullable outputs use `anyOf`. | `tool_schemas_use_one_type_per_value`, Inspector `--strict` |
| ReMa MCP errors carried `structuredContent`, which the TypeScript SDK checks against the output schema, so clients saw a schema failure instead of the reason. | Errors are text with `isError`. | same test, Inspector call |

## What to run with the missing inputs

On a Mac with ReMa built by `pnpm build:app`:

```sh
REMA_LIVE_ANTHROPIC_API_KEY=… REMA_LIVE_OPENAI_API_KEY=… REMA_LIVE_GEMINI_API_KEY=… \
REMA_LIVE_CODEX_HOME="$HOME/Library/Application Support/cloud.datasound.rema/runtimes/codex" \
  cargo test --locked --lib live_ -- --ignored --nocapture --test-threads 1
```

Each prints `VERIFIED …` with its evidence, or `FAILED …` with the reason.
In the app: connect Gmail, Google Calendar, Outlook Mail and Calendar;
turn one off in a chat's **+** menu; restart; and use **Settings →
Career Search → Check now** with each provider as the default model.
