# Connectors, MCP and web search parity: validation

What was run after the changes in [implementation.md](implementation.md),
and what was observed. Environment: Linux container, Xvfb display, D-Bus
session with GNOME Keyring, debug build with the frontend bundled
(`pnpm tauri build --debug --no-bundle`). No live provider accounts were
available; the in-app runs used the stand-ins in
`scripts/e2e/mock-providers.mjs`, which log every model request (tools,
web search settings, and whether private data or LinkedIn members reached
a model). The app ran with a scratch `HOME` and `XDG_CONFIG_HOME`, so no
real Claude or Cursor configuration was read.

## 1. Automated checks

| Check | Result |
|---|---|
| `cargo fmt --check` | clean |
| `cargo clippy --all-targets --locked -- -D warnings` | clean |
| `cargo test --locked` | 772 passed, 3 ignored |
| `pnpm lint`, `pnpm typecheck` | clean |
| `pnpm test` | 139 passed (29 files) |

New tests by area:

| Area | Tests |
|---|---|
| Per-chat connectors | `connectors_that_are_on_come_with_the_web_until_private_data_is_read`, `web_search_stops_once_a_tool_returned_private_data`, `a_search_held_next_to_a_private_tool_is_dropped_with_the_web` (HTTP), `a_search_that_has_not_run_is_left_out_once_the_web_is_off`, `a_claude_sign_in_sends_every_beta_in_one_header`, `codex_and_gemini_answers_get_either_connectors_or_their_search`; Vitest `chatSelection`, `ChatToolsMenu` |
| Staying signed in | `connected_accounts_are_kept_signed_in_without_reading_anything`, Google `invalid_grant` hint in `failure` tests |
| Native search | `models_with_their_own_search_answer_like_chatgpt_and_claude`, `the_models_own_search_is_the_default_and_the_choice_is_kept`; the pipeline tests now run with ReMa verified search; Vitest `CareerSearchSection` |
| Analytics guard | `job_tables_the_model_did_not_look_up_stay_out_of_analytics` (chat and scheduled task) |
| MCP environment | `local_servers_get_the_users_environment_and_their_own`, `reads_the_login_environment_between_its_markers`, `the_login_path_comes_right_after_the_programs_own_folder` |
| MCP import | `mcp::import` (formats, `${VAR}`, headers, SSE, shells, JSONC, Claude Code projects, no values in candidates), `adds_servers_from_another_apps_configuration` (end to end with a real stdio server); Vitest `McpImportDialog` |
| ReMa MCP in other apps | `another_app_lists_and_calls_rema_mcp_over_stdio`, `does_not_start_while_turned_off_in_settings`, `adds_itself_to_another_apps_settings_and_keeps_its_servers`; Vitest `remaMcpSnippets` |
| Contacts | `network::contacts` (Connections.csv with notes, ZIP, vCards, replace/upsert, no e-mail kept), `imported_contacts_show_whom_you_know_without_linkedins_api`; Vitest `NetworkContactsCard` |

## 2. In-app runs

| Scenario | Observed |
|---|---|
| Start with Gmail and Google Calendar connected | Log: `[oauth] provider=google phase=token_refreshed` at start (keep-alive), no mail read. |
| Chat **+** menu | Connectors group: Gmail, Google Calendar, Applications, all on; "Connector settings…"; ReMa MCP below. |
| Gmail turned off, Gemini 3 Flash, "Which of my applications need action, and do I have interviews this week?" | Request: `calendar_*` and `applications_*` tools, no `mail_*`, no Google Search (Gemini keeps its search for a whole answer), ReMa MCP tools. Conversation stored `["google_calendar","applications"]`. |
| Same chat, "Find current AI Engineer jobs in Vienna" | Request: Google Search with `includeServerSideToolInvocations`, ReMa MCP tools, no connector tools, no ReMa search step. |
| OpenAI (API key), "Find current AI Engineer jobs in Vienna, posted within the last 10 days." | One request: `web_search` with `allowed_domains: 0`, `user_location: Vienna`, sources included, ReMa connector and ReMa MCP functions next to it; the model searched and answered with its own table ("Searched the web · 1 search"); "Analyze 2 jobs" offered (the answer searched, so its jobs reached Analytics). |
| Settings → MCP → Import from other apps | Found the scratch Claude Desktop file; `notes-fixture` selectable with its variable names; `shell-wrapped` shown with "It starts through a shell…" and not selectable. Added and connected at once (3 tools, MCP 2026-07-28). Database: names only; the keychain entry holds the value of `${MCP_E2E_TOKEN}` filled in. |
| ReMa MCP → Use in other apps → Add to Claude Desktop | `mcpServers.rema = { command: <ReMa>, args: ["mcp"] }` written; `globalShortcut` and the other servers kept; `.rema-backup` created. That entry, started as Claude Desktop would, answered `initialize`, listed the 5 tools and ran `search_jobs` while the app was running on the same data. |
| Settings → Connectors → Your contacts | LinkedIn export ZIP picked in the system file dialog: "Imported 2 contacts", "2 LinkedIn connections". vCard: Lukas Gruber (Donau Data GmbH, XING profile link). Stored rows have no e-mail addresses. |
| Anthropic (API key, Claude 5), "Find current AI Engineer jobs in Vienna" | One request: `web_search_20260318` and `web_fetch_20260318` with no domain list and `user_location: Vienna`, next to the connector and ReMa MCP tools; Claude searched and wrote the answer itself. |
| Settings → Career Search → ReMa verified search, same question | The pipeline ran instead: a jobs step with Anthropic's tools kept to 20 career sites, ReMa's sources and the two postings opened, then an answer step without tools. |
| Network Connect with imported contacts | Jane Example shown as "LinkedIn connection (from your data export)", with "From the contacts you imported: kept on this computer and never sent to a model." |
| Leak check over the run | 6 model requests (Gemini 2, OpenAI 1, Anthropic 3): none carried mail text, imported contacts or LinkedIn members. No token, key or refresh token in the logs or the database. |
| Settings → Connectors, XING | A plain note that XING has no API for ReMa, pointing to Your contacts; not shown as a fault. |

## 3. Found while validating

Anthropic's docs describe two requests the mid-answer web switch-off could
make that the API rejects:

- Claude can ask for a search and a ReMa tool in the same step; the API
  holds the search until the next request and rejects that request if the
  search tool is no longer declared ("…but no `web_search` tool was
  provided"). ReMa now leaves the held search out of the step it sends
  back and shows it as not run.
- Claude 5 models bind each thinking block to the tools it was written
  with (enforced for accounts created from 31 August 2026). The request
  after the switch-off now asks the API to drop that earlier thinking
  (`thinking.block_binding.prefix_mismatch_behavior: "drop_block"`, beta
  `thinking-binding-controls-2026-08-01`) instead of refusing it; the
  step's calls and results stay.

Both are covered by the tests above; neither can be shown with the
stand-ins, which do not enforce these rules.

## 4. Not verified here

- Live providers and accounts: no live OpenAI, Anthropic, Gemini, Google,
  Microsoft, LinkedIn or XING accounts. Request shapes follow the sources
  in implementation.md and are enforced by adapter tests and the
  stand-ins.
- Claude Desktop, Cursor, VS Code and Codex themselves are not installed
  here; their configuration files and the stdio launch were exercised as
  those apps use them.
- The login-shell environment on macOS (`zsh -ilc`): covered by the parser
  test and the Linux run; not run on a Mac.
