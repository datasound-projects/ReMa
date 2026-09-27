# Harness audit — findings and repairs

A full audit of ReMa against its specs: the LLM layer and tool loop, web
and career search, connectors and mail sync, provider settings, the
frontend event flow, persistence and the desktop lifecycle. Provider
behavior was checked against current official documentation (Anthropic's
web search and web fetch pages and Claude 5 model guidance; OpenAI's web
search and reasoning-item guidance; Google's rules for combining built-in
tools with function calling, confirmed through public GitHub issues and
pull requests because Google's documentation host is not reachable from
the audit environment). Validation: [validation.md](validation.md).

## Findings

| # | Issue | Area | Severity | Root cause |
|---|---|---|---|---|
| 1 | Every Gemini 3 chat with tools failed (400 "Please enable tool_config.include_server_side_tool_invocations…") | `llm/gemini.rs` | Critical | Google Search next to function declarations without `toolConfig.includeServerSideToolInvocations`; the error matched no fallback pattern. ReMa MCP is on by default, so every chat carries functions. |
| 2 | Claude 5 thinking used up small output caps (mail triage, classification, search steps cut off) | `llm/anthropic.rs` | High | `max_tokens` was the caller's cap; Claude 5-series models think by default and thinking counts against it. |
| 3 | Long chats failed on every new message | `services/chat.rs` | High | The whole history was sent; nothing trimmed it. |
| 4 | Outlook with more than 2,000 changes never synced | `connectors/microsoft/mail.rs` | High | After 40 pages the read failed and kept no position; `@odata.nextLink` was not kept as the continuation. |
| 5 | Answers cut off at the output limit looked complete | chat, scheduler, `jobs::ask` | Medium | `Finish::MaxTokens` was never read. |
| 6 | Retry without tools kept a prompt that promised web search and tools | `services/chat.rs` | Medium | The system prompt was built once. |
| 7 | Scheduled prompt tasks always failed on local models without tool support | `services/scheduler.rs` | Medium | The no-tools fallback existed only in chat. |
| 8 | Tool call ids repeated across rounds (activity overwritten, approvals keyed alike) | `llm/mod.rs` | Medium | Generated ids restarted at `call_0` each round. |
| 9 | The 8-round tool limit threw the answer away | `llm/mod.rs` | Medium | No final round without tools. |
| 10 | One 429, 5xx or 529 before any output failed the answer | `llm/mod.rs` | Medium | Requests were sent once. |
| 11 | `<think>…</think>` from local models reached chat, history and JSON parsers | `llm/openai.rs` | Medium | Not filtered. |
| 12 | Gmail's first sync and backfill silently skipped mail beyond 200 messages | `gmail.rs`, `jobs/mod.rs` | Medium | The cap was not reported; coverage was recorded as the whole lookback. |
| 13 | Private mail in a chat could reach web-enabled answers in later turns | `services/chat.rs` | Medium | The privacy boundary was checked per message, not per chat. |
| 14 | Settings showed "Connected" after the provider rejected the API key | `services/providers.rs`, settings UI | Medium | Only billing errors were recorded as provider health. |
| 15 | No Gemini end-to-end stand-in | `scripts/e2e/mock-providers.mjs` | Medium | Harness gap (why #1 went unnoticed). |
| 16 | OpenAI reasoning items were dropped between tool rounds | `llm/openai_responses.rs` | Low | Only text and function calls were kept. |
| 17 | Anthropic web fetch had no size limit | `llm/anthropic.rs` | Low | `max_content_tokens` not set. |
| 18 | A Claude Console token could expire during a long approval wait | `llm/mod.rs` | Low | The credential was captured once per answer. |
| 19 | Scheduled tasks on local models ignored the optional search service | `services/scheduler.rs` | Low | Hard-coded `None`. |
| 20 | The chat view buffered other conversations' stream events | `src/hooks/useChat.ts` | Low | No conversation filter. |
| 21 | A Gemini key Google no longer accepts was a generic error, never shown in Settings (found during repair) | `llm/http.rs` | Medium | Google answers with 400 `INVALID_ARGUMENT` / `API_KEY_INVALID`, not 401/403. |
| 22 | After a restart, Settings showed a rejected API key as "Connected" again (found in the app run) | `services/providers.rs` | Medium | Provider health lived only in memory. |
| 23 | A local model in a chat that read private data lost ReMa's web tools without a word (found in the app run) | `services/chat.rs` | Low | The notice was tied to the provider's own web search, which compatible endpoints do not have. |
| 24 | Settings told an API-key user to "Reconnect … in Settings" (found in the app run) | `CloudProviderRow.tsx`, `llm/http.rs` | Low | One generic backend message for keys and account sign-ins. |

Checked and found working: Anthropic web tool versions and `pause_turn`
handling, OpenAI web search parameters, the locked-down Codex runtime,
model discovery, MCP transports and approvals, local models' ReMa search
tools, the connector token manager, loopback OAuth and the release gate,
start-up crash recovery, migrations, debug-only overrides, single
instance.

## Repairs

**Gemini (1, 8, 15, 21).** `gemini::combines_search_with_functions` is true
for Gemini 3 and later (and version-less `gemini-` aliases). Those requests
carry `includeServerSideToolInvocations: true`; every part of the model's
turn (text, thought signatures, `toolCall`/`toolResponse`, `functionCall`
with its `id`) is kept in `StreamState.blocks` and sent back verbatim;
`functionResponse` carries the call's id. Gemini 2.x never gets Google
Search next to functions: the chat gives it ReMa's search tools instead.
The fallback also recognizes Google's error text. A 400 whose message says
the API key is not valid or expired is an authentication error.

**Anthropic (2, 17).** `thinks_by_default` (Opus/Sonnet/Haiku 5 and later,
Fable, Mythos) adds 16,000 tokens of headroom to `max_tokens`. Web fetch
has `max_content_tokens: 20000`.

**Tool loop (8, 9, 10, 18).** Generated ids are `rema_call_<round>_<n>`;
Gemini gets only provider ids back. After 8 tool rounds the request is sent
once more with `tools_off` (`tool_choice: none` / Gemini
`functionCallingConfig: NONE`). `http::send_retrying` retries 408, 429, 5xx,
529 and connection failures twice (1 s, 3 s, or `retry-after` up to 20 s)
before anything streamed, never an empty quota. The Claude Console
credential is read from its runtime before each request.

**OpenAI (16) and local models (11).** Reasoning items with non-empty
`encrypted_content` are passed back before the round's calls
(`store: false`). `ThinkFilter` removes inline reasoning even when a tag is
split across stream chunks.

**Chat (3, 5, 6, 13).** `fit_history` keeps the newest turns within
200,000 characters (16,000 for OpenAI-compatible endpoints) and the prompt
says older turns were left out. A cut-off answer ends with a note. When a
model refuses tools, the system prompt is rebuilt with web access off and
a notice is shown; the provider is not marked as refusing web search. Once
a chat has read private data (connector activity), later answers in it
have no web access or web tools, MCP calls need approval, and the chat says
so, also for local models (23). Job, company, network and business
searches in such a chat are built from the new message only; their answer
steps get the history but no web access or tools.

**Scheduler (5, 7, 19).** Prompt tasks share the chat's no-tools fallback
and honest prompt, pass the configured optional search service, and add the
cut-off note. Every branch notes the model call's outcome.

**Mail sync (4, 12).** `SyncBatch` carries `unread_before` and `more`.
Outlook returns a partial batch after 40 pages with the `nextLink` as its
cursor; a round without either link is an error. Gmail reports when a read
stopped at 200 messages. `sync_mailbox` records coverage only as far back
as it read, reports why, and later runs backfill the rest by date.
`jobs::ask` treats a cut-off reply as an error, so the email is retried.

**Provider state (14, 22, 24).** `providers::note_outcome` records
authentication failures as `ReauthRequired` with the reason and clears them
after the next successful request (or a saved endpoint). For API-key
connections the rejection is also stored (`provider.<id>.key_rejected`, the
message only, never the key), so it survives a restart until a request
succeeds, the key is replaced or the provider is removed; account sign-ins
are checked with their runtime at start instead. Settings shows **Replace
key** and "no longer accepts this API key" for API keys; custom endpoints
say to edit the key. Gemini's rejection keeps Google's reason ("Gemini
rejected the API key: API key not valid. …").

**Frontend (20).** `useChat` ignores stream events of other conversations
once its own id is known.

**Harness.** The E2E stand-ins enforce the documented provider rules
(Gemini's tool combinations and verbatim turns, an Ollama-like server that
refuses tools, Gmail list paging) and give Gmail messages whole-millisecond
`internalDate` values like the real API.

## Files

Rust: `llm/{mod,http,gemini,anthropic,openai,openai_responses,fake,tests_http}.rs`,
`services/{chat,scheduler,providers,connector_tools}.rs`,
`retrieval/native.rs`, `jobs/{mod,tests}.rs`, `connectors/mail.rs`,
`connectors/google/gmail.rs`, `connectors/microsoft/mail.rs`, `lib.rs`.
Frontend: `components/settings/{CloudProviderRow,ProviderRows}.tsx` (+ tests),
`hooks/useChat.ts` (+ test). Harness: `scripts/e2e/mock-providers.mjs`.

## Limitations (not fixed)

- Gemini's grounding sources are redirect links; postings Gemini lists are
  kept only when ReMa reads their page itself (stricter, not wrong).
- The DuckDuckGo route may be closed by its robots rules; it is one route
  of several and a failure is reported.
- No live provider accounts were available: provider behavior is verified
  against documentation, adapter tests and stand-ins that enforce the
  documented rules.
