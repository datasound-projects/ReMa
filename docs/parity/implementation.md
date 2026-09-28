# Connectors, MCP and web search: parity with Claude and ChatGPT

What was asked: connect Google and Outlook once and have them work
whenever they are turned on, as in the Claude and ChatGPT apps; connect
LinkedIn and XING where the user has an account; make ReMa MCP and other
MCP servers work the way Claude Code and other harnesses run them; and let
OpenAI and Anthropic models answer with their own web search, so the
result in ReMa matches ChatGPT and Claude. Validation:
[validation.md](validation.md).

## Sources (read September 2026)

| Topic | Source |
|---|---|
| Claude connectors (connect once, per-chat toggles) | [Use Google Workspace connectors](https://support.claude.com/en/articles/10166901-use-google-workspace-connectors) |
| ChatGPT connected apps | [Connected apps in ChatGPT](https://help.openai.com/en/articles/11487775-connected-apps-in-chatgpt) |
| Google refresh tokens (7 days in Testing, about 6 months unused) | [Using OAuth 2.0](https://developers.google.com/identity/protocols/oauth2), [Production readiness](https://developers.google.com/identity/protocols/oauth2/production-readiness/overview) |
| Microsoft refresh tokens (90 days, rotating) | [Refresh tokens in the Microsoft identity platform](https://learn.microsoft.com/en-us/entra/identity-platform/refresh-tokens) |
| Web search next to private data (exfiltration) | [Web search tool](https://platform.claude.com/docs/en/agents-and-tools/tool-use/web-search-tool), [Web fetch tool](https://platform.claude.com/docs/en/agents-and-tools/tool-use/web-fetch-tool) |
| Server tools held next to client tools; tools that must stay declared | [Server tools](https://platform.claude.com/docs/en/agents-and-tools/tool-use/server-tools) |
| Thinking bound to the tools it was written with (`drop_block`) | Anthropic model migration guide, "preserved thinking" (Claude Opus 5.5, Claude Fable 5.1) |
| OpenAI web search | [Web search](https://platform.openai.com/docs/guides/tools-web-search) |
| Gemini Google Search with function calling | [Grounding with Google Search](https://ai.google.dev/gemini-api/docs/google-search) |
| MCP protocol revisions | [Versioning](https://modelcontextprotocol.io/specification/versioning) |
| How Claude Code runs MCP servers (environment, `${VAR}`, timeouts, output cap, import) | [Connect Claude Code to tools via MCP](https://code.claude.com/docs/en/mcp) |
| LinkedIn: Connections API restricted; the member's data export has `Connections.csv` | LinkedIn developer and help pages (search results; `linkedin.com` pages are not reachable from the build environment) |
| XING: no API for new applications; contacts as vCards | XING help pages (search results) |

## Findings and changes

| # | Finding | Change |
|---|---|---|
| 1 | Connector tools were offered only when a question looked private; there was no per-chat choice. | Connectors are on in every chat and the **+** menu turns one off for that chat (migration 0014); the model decides when to use them. |
| 2 | With connectors, the answer had no web at all. | The web stays until a tool returns private data. OpenAI and Anthropic drop their hosted search for the rest of the answer; ReMa's web tools refuse; MCP calls need approval. Codex and Gemini (search fixed for a whole answer) get either the connectors or their search per answer. |
| 3 | A grant that goes unused expires (Microsoft 90 days, Google about 6 months). | Accounts are renewed at start and daily while ReMa runs (a token refresh only). A Google `invalid_grant` explains the 7-day limit of apps in Testing. |
| 4 | Job, company and people questions always went through ReMa's search-first pipeline, so OpenAI and Anthropic answers differed from ChatGPT and Claude. | Models whose provider hosts a search (OpenAI API, ChatGPT through Codex, Anthropic, Gemini 3) search themselves and write the answer, not kept to career sites; ReMa MCP sits next to the search without a second nested search by the same provider. Local models, Gemini 2 and refused providers keep the pipeline. Settings → Career Search can choose ReMa verified search for every model. Scheduled tasks follow the same choice. |
| 5 | Any job table in an answer reached Analytics. | Only answers and runs that looked something up are ingested. |
| 6 | Local MCP servers got a minimal environment, so tokens, proxies and version managers from the user's shell profile were missing (worst when ReMa was opened from the Finder or the Dock). | The full environment plus the login shell's (read once, as VS Code does), like Claude Code; ReMa's own `REMA_*` settings are not passed on. |
| 7 | 45 s to start and 180 s per tool call were too short for first `npx`/`uvx` runs and long tools. | 90 s and 10 minutes; Stop still ends an answer at once. |
| 8 | Servers had to be re-entered by hand. | Import from Claude Desktop, Claude Code, Cursor, VS Code, Windsurf or pasted JSON, with `${VAR}` expansion and clear reasons for what cannot be added (shell wrappers, legacy SSE, several headers, VS Code inputs). |
| 9 | ReMa MCP worked only inside ReMa. | `ReMa mcp` serves it over stdio to other apps from the app's data folder; Settings adds it to Claude Desktop, Cursor and VS Code in one click and shows setups for Claude Code and Codex. Turning it off in ReMa stops it there. |
| 10 | Without LinkedIn's approval for its Connections API, and with no XING API, Network Connect could never tell whom the user knows. | The user imports their own LinkedIn data export and vCards (migration 0015). Only name, company, position, profile link and connection date are kept; the data is matched locally, never sent to a model, copied out or used in Business. |
| 11 | Switching Anthropic's web tools off mid-answer could make the API refuse the next request: a search Claude asked for next to a ReMa tool was still pending, and Claude 5 thinking is bound to the tool list. | The pending search is left out and shown as not run; on Claude 5 models the request asks the API to drop the earlier thinking (`drop_block`) instead of refusing. |

## Decisions

- **Why web search is dropped mid-answer, not per chat.** Anthropic warns
  that web tools next to untrusted private data can leak it. Dropping the
  hosted tool after the round that returned private data keeps the web for
  the first part of an answer (as in Claude and ChatGPT) without letting a
  later round search with private data. Only APIs that take a request's
  tools anew each round (OpenAI Responses, Anthropic Messages) allow that.
  On Anthropic this changes the tool list mid-answer, which Claude 5
  models treat as editing the conversation: the request that follows asks
  the API to drop the thinking written before the change, the documented
  way to degrade rather than fail.
- **Why a setting for verified search.** ReMa's pipeline opens every
  posting it lists; the provider's own search does not promise that. The
  default follows the request (parity); the setting keeps the stricter
  mode one click away.
- **Why the full environment for MCP servers.** It is what Claude Code
  does and what most servers' setup instructions assume. ReMa keeps no
  secrets in its environment (they are in the OS credential store).
- **Why import rather than scraping for LinkedIn and XING.** Their terms
  allow neither scraping nor, without approval, API access to connections;
  the member's own export is theirs to use.

## Files

Rust: `services/{chat,scheduler,mcp,connector_tools}.rs`,
`career_search/{mode,status}.rs`, `mcp/{client,import}.rs`,
`rema_mcp/{engine,host,stdio}.rs`, `network/{contacts,relationships,service,policy,capabilities}.rs`,
`connectors/{tokens,failure,xing}.rs`, `accounts/locate.rs`, `llm/mod.rs`,
`events.rs`, `lib.rs`, migrations 0014 and 0015.
Frontend: `ChatToolsMenu`, `ChatPage`, `useChat`, `chatSelection`,
`CareerSearchSection`, `McpImportDialog`, `RemaMcpElsewhere`,
`NetworkContactsCard`, `lib/remaMcpSnippets.ts`. Harness:
`scripts/e2e/mock-providers.mjs` (a model that searches itself).
