# ReMa MCP — implementation record

ReMa MCP is ReMa's built-in, first-party job-search MCP server: five read-only
tools (`search_jobs`, `get_job`, `get_jobs`, `search_similar_jobs`,
`source_status`) that find vacancies, read job descriptions from permitted
sources, and return compact, source-linked results to the chat model.

This file records the integration points, decisions, source permissions,
and the requirement checklist. The validation report (commands, results,
measurements) is in [validation.md](validation.md).

## 1. Integration points (Phase 1)

| Area | Where | Notes |
|---|---|---|
| MCP client | `src-tauri/src/mcp/client.rs` | Official Rust SDK `rmcp` 3.4.1. Prefers protocol **2026-07-28** (`server/discover`, stateless core) and falls back to the **2025-11-25** `initialize` handshake. |
| Chat tool dispatch | `src-tauri/src/services/chat_tools.rs` | Tools are offered as `mcp_<server>_<tool>`; read-only tools run without an approval prompt, others wait for Allow/Deny. |
| Chat request assembly | `src-tauri/src/services/chat.rs` (`generate`) | Job-search messages first go through the enforced retrieval workflow (`src-tauri/src/retrieval/`); all other messages get MCP tools, then ReMa's web tools for local models. |
| Search backends | `src-tauri/src/retrieval/backend.rs`, `retrieval/native.rs` | The user's search service (Brave / Tavily / SearXNG, key in the OS keychain) and the chat model's provider-hosted web search. |
| Safe page reading | `src-tauri/src/analytics/page.rs` | Public-address check, manual redirects, size limit, JSON-LD `JobPosting` parser. The DNS check happens before connecting (not at connect time); ReMa MCP adds a connect-time resolver. |
| Settings | `src-tauri/src/db/providers.rs` (`settings` table), `src/components/settings/McpSection.tsx` | |
| Chat `+` menu | `src/components/chat/ChatToolsMenu.tsx` | Per-chat selection of user-added servers. |
| Agents | `src-tauri/src/services/agents.rs` | Built-in Agents are code constants (updated in place). |
| Persistence | `src-tauri/src/db/migrations/*.sql` | Versioned, additive migrations. |
| Packaging | `src-tauri/tauri.conf.json`, `vite.config.ts` | Frontend source maps are emitted only for debug builds (`TAURI_ENV_DEBUG`). Release profile: LTO, `strip = true`. The local-fixture switches (`REMA_DEV_*`) exist only in debug builds. |

Baseline before this work (commit `c6b1700`): `cargo fmt --check` clean,
`cargo clippy --all-targets -D warnings` clean, `cargo test` 337 passed
(2 ignored), `pnpm test` 53 passed, `pnpm typecheck`, `pnpm lint`,
`pnpm build` clean.

## 2. Decisions (Phase 2)

### Protocol, SDK, transport

- **SDK:** `rmcp` 3.4.1 (already ReMa's client SDK). The server side uses the
  same crate (`server` feature). One SDK version on both ends removes
  revision mismatches; the client still negotiates 2026-07-28 first.
- **Transport: in-process.** ReMa MCP runs inside the ReMa binary. Each chat
  answer opens a session over an in-memory byte stream
  (`tokio::io::duplex`), carrying the same newline-delimited JSON-RPC
  framing as stdio, between ReMa's MCP client and the ReMa MCP server.
  - Nothing to install and no sidecar process: nothing to orphan, no local
    network listener, no executable path or port to show.
  - The implementation is compiled into the release binary (LTO, stripped);
    no JavaScript implements job search.
  - This deviates from "prefer a bundled stdio sidecar", which the brief
    applies to a separate local process. Tool calls still pass the real MCP
    boundary: discover or initialize, `tools/list`, `tools/call` with
    `structuredContent`.
- **No recursion:** discovery through the chat model's hosted web search
  sends a bounded request (role, place and constraints only; no chat
  history, no CV, no tools). ReMa MCP is never offered to that request.

### Activation (implementation override)

- Built in and **enabled by default**: the setting `rema_mcp.enabled` is
  written as `true` on first launch if absent and is never overwritten by
  migrations or updates.
- While enabled, its tools are offered in **every chat** automatically (no
  per-chat selection, no chip). Nothing runs until the model calls a tool
  for the user's request; there is no background crawl.
- **Disable:** Settings → MCP → Built-in, two consecutive confirmations. The
  backend then removes the tools from new requests, rejects new or queued
  calls, and cancels in-flight ReMa MCP work. Saved jobs, chats and
  Analytics data are untouched. Re-enabling takes one click.
- User-added MCP servers keep their behavior (per-chat selection,
  approvals).

### Search backend (`SearchBackend`)

In the order the brief prescribes:

1. **The user's configured search service** (Settings → Web search: Brave
   Search API, Tavily, SearXNG), an existing ReMa integration with
   structured results. Site-scoped queries use `site:` (Brave, SearXNG) or
   `include_domains` (Tavily).
2. **The chat model's provider-hosted web search** (ChatGPT/Codex, OpenAI
   `web_search`, Anthropic `web_search`, Gemini Google Search), through the
   narrow host request above. It is billed to the user's own provider
   account; no new key is needed.
3. **Neither:** `search_jobs` returns `SEARCH_BACKEND_UNAVAILABLE`.
   `get_job` / `get_jobs` for supported URLs still work.

No shared secret is embedded in the binary.

### Source registry and acquisition modes

The registry is in `src-tauri/src/rema_mcp/sources.rs`.

| Source | Mode | Discovery | Content | Basis / restriction |
|---|---|---|---|---|
| Greenhouse | `documented_public_feed` | search results | Job Board API `GET boards-api.greenhouse.io/v1/boards/{board}/jobs/{id}` | Public published-board GET endpoints only; never the application API. EU-hosted boards (`job-boards.eu.greenhouse.io`) are read as pages (EU API host not verified). |
| Lever | `documented_public_feed` | search results | Postings API `GET api.lever.co/v0/postings/{site}/{id}` (`api.eu.lever.co` for `jobs.eu.lever.co`) | Known site only; not a global index. |
| Ashby | `documented_public_feed` | search results | Job Postings API `GET api.ashbyhq.com/posting-api/job-board/{board}?includeCompensation=true` | Only listed jobs; board fetched once per 10 minutes. |
| Personio | `documented_public_feed` | search results | XML feed `GET {company}.jobs.personio.de/xml` | Parsed with `quick-xml`, which does not expand DTDs/external entities. |
| Employer career pages | `permitted_public_page` | search results | One GET of the page; `JobPosting` JSON-LD, else main text | Honors `robots.txt`; public addresses only; no scripts, no cookies, no logged-in content. |
| LinkedIn | `search_discovery_only` | indexed links from the search backend | none | The Job Posting API is for publishing; the User Agreement restricts scraping. Links are labeled as discovered, never fetched. |
| XING | `search_discovery_only` | indexed links | none | No verified candidate-side search endpoint or authorization. |
| Regional boards (karriere.at, jobs.at, StepStone, Indeed, Glassdoor, jobs.ch, jobup.ch, Pracuj.pl, No Fluff Jobs, Just Join IT) | `search_discovery_only` | indexed links | none | Terms not reviewed for automated reading; kept as discovery links. |
| SmartRecruiters, Workable, Teamtailor, Workday, SuccessFactors, Oracle/Taleo, iCIMS | evaluated: no dedicated adapter | search results | as employer pages when their job pages carry `JobPosting` markup | No adapter stubs. |

**Documentation note:** this sandbox's egress policy blocks the official
documentation hosts (modelcontextprotocol.io, developers.greenhouse.io,
github.com/lever, developers.ashbyhq.com, developer.personio.de, …) and the
APIs themselves. The endpoints above follow the vendors' public
documentation as known at implementation time. They are covered by fixture
tests but **not verified against the live services in this environment**.
Recheck them before release.

### Budgets (initial ReMa defaults, not measured limits)

| Control | Value |
|---|---|
| Results | 20 by default, 50 per page at most |
| Batch | 10 references per `get_jobs` |
| Search-backend queries | 8 per search, including canonical lookups |
| Candidates / fetches | 100 links / 30 detail fetches per search |
| Concurrency | 6 in total, 2 per host |
| Deadline | 20 s per search with a search service. **90 s** when discovery runs through the chat model's hosted web search: one model turn with searches commonly takes longer than 20 s. This is a documented, host-controlled exception that `source_status` reports. |
| Fetch safety | 5 redirects, 2 MiB per page, 8 MiB per ATS feed, ports 80/443 only |
| Cache | Search snapshots 10 min; status checks 30 min |
| Payload | Compact search output ≤ 32 KiB; job descriptions in 12,000-character parts with a continuation cursor |

## 3. Requirement checklist

Status: `complete` (implemented and tested), `blocked` (external
access/permission), `not run` (check not executed). Evidence is in
[validation.md](validation.md).

| # | Requirement | Status | Evidence |
|---|---|---|---|
| 1 | Built in, enabled on first launch, no install/config | complete | `enabled_by_default_and_the_saved_choice_survives_restarts`; debug and release app on a fresh data directory; release app on a schema-6 database (update path) |
| 2 | Tools automatically available in every compatible chat | complete | `rema_mcp_is_offered_in_every_chat_until_it_is_turned_off`; in-app chats (debug and release) |
| 3 | Two-step disable, Escape/close cancels, persists, one-click re-enable | complete | `RemaMcpCard.test.tsx`; in-app run with the stored value read after each step and after a restart |
| 4 | Disabled: tools removed, calls blocked in the backend, in-flight work cancelled | complete | `disabling_blocks_calls_and_cancels_running_work_but_keeps_saved_jobs`; in-app chat while off |
| 5 | Five tools with typed input/output schemas and domain error codes | complete | `serves_five_tools_over_mcp_and_finds_jobs_end_to_end`, `reads_jobs_in_detail_one_or_many` |
| 6 | Greenhouse, Lever, Ashby, Personio, employer JSON-LD adapters | complete against fixtures; live unverified (#15) | adapter unit tests; end-to-end tests |
| 7 | LinkedIn/XING discovery-only, never fetched | complete | end-to-end tests (request log), `BLOCKED_BY_SOURCE_POLICY` for LinkedIn URLs; in-app mock log |
| 8 | SSRF protections (schemes, credentials, private IPs, DNS rebinding, redirects, size) | complete | `sources.rs` and `fetch.rs` tests; release app refused the local fixture pages |
| 9 | Strict filters, unknown ≠ match, no silent relaxation, salary comparability | complete | `filter.rs` and `extract.rs` tests |
| 10 | Conservative deduplication, stable IDs, provenance | complete | end-to-end tests (possible-duplicate notes, evidence, `rj_` ids) |
| 11 | Cache, snapshots, cursors, truthful refresh, stale warnings | complete | `cached_searches_cursors_and_truthful_refresh`, `cursors_round_trip_and_reject_tampering` |
| 12 | Untrusted content handling, no Profile database reads | complete | injected instructions in fixture pages are returned as data and their links are never requested; `never_reads_profile_data` |
| 13 | Settings Built-in card, readiness separate from enabled state | complete | card tests; screenshots of Ready / Search setup required / Off |
| 14 | Agents updated | complete | Job Search Agent and Job Match Analyst instructions in `services/agents.rs` |
| 15 | Live source checks | blocked (egress policy) | live sample size 0 |
| 16 | Release packaging | complete | `pnpm tauri build --no-bundle`; no source maps; no dev switches in the binary; release app checked on Xvfb |
