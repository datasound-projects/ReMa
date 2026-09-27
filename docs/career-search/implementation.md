# Always-On Career Search — implementation record

Career search is a built-in ReMa capability, not a user-configured
integration: a request for current jobs, companies, people or market facts
always has a search route, whatever model is selected and whether or not a
search service is set up. Specification:
`2968f2d9-ReMa_Always_On_Career_Search_Infrastructure.md` (kept outside the
repository). The plan and checklist for the three current specifications are
in [../implementation-plan.md](../implementation-plan.md); validation is in
[validation.md](validation.md).

## 1. Where things live

```text
career_search/mod.rs        the shared runtime (AppState.career): source health, route reports
                            (last 50), per-server capability cache, company cache (24 h),
                            learned boards; UNAVAILABLE (the one operational failure text)
career_search/requirement.rs   Requirement None / Optional / Required and Scopes (jobs,
                            company, people, market) from the request text (§20–§22)
career_search/plan.rs       SearchPlan: roles and their variants, place (city, country,
                            ISO code, time zone), remote, minimum salary, recency,
                            companies, people; Hints (domain filter, approximate location)
career_search/registry.rs   the career source registry: ATS, boards, networks, company and
                            market sites, by scope and region; allowed_domains (≤ 20) (§23)
career_search/router.rs     search_jobs and research: every route at once, merged (§4–§6)
career_search/jobs.rs       ReMa's no-key job sources: employers' boards (named companies,
                            learned boards), public boards; health-gated, concurrent
career_search/company.rs    Wikidata (current statements only), Wikipedia, the company's
                            own site (homepage, team, careers → ATS boards)
career_search/research.rs   findings with source kinds, contact redaction, evidence from
                            postings, the model context block and the Sources list
career_search/health.rs     per-source circuit breakers (3 failures → rest, backoff to 15 min;
                            rate limits rest at once)
career_search/status.rs     Settings status, the health check, start-up capability detection
rema_mcp/adapters/          + SmartRecruiters, Workable, Recruitee list APIs; boards.rs:
                            Arbeitnow, The Muse, Remotive, Hacker News "Who is hiring?"
rema_mcp/engine.rs          Discovery = ReMa's sources ∥ optional service ∥ provider search;
                            fails only when no route answered; pre-read records
retrieval/native.rs         the provider's own search: required, domain filter, location,
                            verified against what the engine reported; research variant
retrieval/listings.rs       page checks, date window, salary status (§27), JSON-LD
retrieval/tools.rs          rema_career_search and rema_read_page for local models
services/chat.rs            jobs → search_then_answer; Required research → research_then_answer
services/scheduler.rs       the same router for scheduled and Run now tasks; run context
src/components/settings/CareerSearchSection.tsx   "Career Search — Automatic"
src/components/settings/AdditionalSearchService.tsx   the optional service (Advanced)
```

## 2. Routing

```text
request ─► requirement + scopes (Required for current jobs, companies, people, market)
        ─► plan (roles + variants, place, salary floor, recency, companies, people)
        ─► hints: career domains of the scopes and region (+ the named companies'
           official domains from Wikidata), approximate location
        ─► at the same time:
             ReMa Jobs      the Jobs MCP over ReMa's no-key sources (+ the optional
                            service, when one is set up and works)
             model search   the provider's own hosted search, when the model has one
        ─► merge: same address, or same employer + role + city; the employer's own
           posting kept ("also listed on …"; a checked duplicate vouches for a page
           that did not open: "checked on …"); employer-less postings left out
        ─► rules: date window, salary floor (stated below → out; not stated → shown
           with a note; estimates never count), closed/expired out
        ─► the table (every row: source, direct address, status and notes), then the
           model writes about those listings only
```

| Selected model | Routes | Model's search request |
|---|---|---|
| OpenAI (API key) | ReMa Jobs + OpenAI web search | Responses `web_search` with `external_web_access: true`, `filters.allowed_domains`, `user_location`, `tool_choice: "required"`, `include: ["web_search_call.action.sources"]` |
| OpenAI (ChatGPT sign-in, Codex) | ReMa Jobs + ChatGPT web search | Codex live web search; the career sites go into the brief as guidance (Codex has no domain filter); its findings stay marked unchecked because Codex reports only opened pages |
| Anthropic | ReMa Jobs + Anthropic web search | `web_search_20260209` + `web_fetch_20260209` (4.6+ and 5-series; `web_search_20250305` before), `allowed_domains`, `user_location`, `max_uses` 8 |
| Gemini | ReMa Jobs + Google Search | grounding with Google Search; ReMa sends no domain filter |
| Unsloth Studio | ReMa Jobs + Unsloth web search | `enable_tools: true`, `enabled_tools: ["web_search"]`, `permission_mode: "off"`, header `X-Unsloth-Events: 1` |
| Other local server | ReMa Jobs; in normal chat the `rema_career_search` / `rema_read_page` tools | — |

A model search counts only when the provider's events show that a search
ran; each finding must be a page the engine reported (Codex excepted, marked
unchecked). A search that did not run is tried once more with a nudge, then
reported as a fallback ("… answered without searching the web … Used ReMa
Jobs instead."). Unsloth is recognised by its model list (`owned_by:
"unsloth-studio"`, asked once per server and remembered 10 minutes); its web
tool is turned on only for ReMa's own search requests, never together with
ReMa's tools, and never with any other of its tools (omitting
`enabled_tools` would enable code execution on the machine).

Research requests (current company, people, market facts — "Who are the
current recruiters at Nordlicht AI?") run `router::research`: ReMa's
sources (Wikidata, Wikipedia intro, the company's homepage, team and careers
pages, its job boards as hiring evidence) and the model's own search in
parallel. The answer request carries the findings in a `<career_sources>`
data block with the instruction to ignore instructions inside it, no tools,
and is followed by the numbered **Sources** list (engine, retrieval time,
kind: official / job posting / professional / reference / market / web).
Emails and international phone numbers are redacted from page text before
anything reaches a model; a finding the engine never reported is dropped.

## 3. No-key sources

| Source | Interface | Used when |
|---|---|---|
| Greenhouse, Lever (+EU), Ashby, Personio, SmartRecruiters, Workable, Recruitee | documented public job-board APIs / feeds | named companies (official site → careers page → ATS links, else name slugs probed), boards learned from earlier results near the place |
| Arbeitnow | public job board API (pages of 100) | DACH places, remote, or no place |
| The Muse | public jobs API (`location=City, Country`) | a city, or remote |
| Remotive | public remote-jobs API (kept 6 hours, as its terms ask) | remote or no place |
| Hacker News "Who is hiring?" | Algolia HN API: latest thread, top-level comments only | always (postings say nothing about still being open; shown as such) |
| Wikidata, Wikipedia | MediaWiki APIs | company facts, official domain, leadership; current statements only |
| Company websites | the hardened fetcher, robots.txt first | homepage description, team and about pages, careers links |

All requests go through `rema_mcp/fetch.rs` (public addresses only, checked
again at connect time; bounded redirects, size and time; binary content
types refused; the page's declared character set honoured). Each source has
a circuit breaker; a failing source never stops the others; a resting source is
skipped; the answer names every source that could not be searched ("Not
reachable right now: … — postings listed only there may be missing"). A repeated search within ten
minutes reuses the Jobs MCP's stored result and is shown with the time its
sources were read.

## 4. Where it is used

- **Chat**: job requests → search, then an assessment of the listings;
  Required research → search, then an answer from the sources; other
  questions to local models get ReMa's career tools; hosted models get their
  own search with the career hints when the request has career scopes.
- **Scheduled tasks** (schedule and Run now): the same router; the run
  records stages ("Searched with ReMa Jobs + … · N searches", "Checked N
  postings (M pages opened) · K matched, L left out") and the context:
  search used, **Search scope** and **Sources consulted**.
- **Agents**: custom agents run in chat, on the same path.
- **Jobs MCP** (`search_jobs` tool): ReMa's no-key sources are its own
  discovery route; the optional service and the model's search are extra
  routes.
- **Network Connect** (Phase 3) calls `router::research` / `search_jobs`.

## 5. Settings

**Career Search — Automatic**: the selected model and whether it searches
itself, the three routes with what they cover, **Check now** (ReMa's job
sources, company research, the model's search), **Recent searches** (route,
results, time, scopes, fallbacks) and **Advanced (optional)** with the
former search-service selector ("None (not needed)"). No state of the
optional service can block a search: a misconfigured service is skipped.

## 6. Official documentation checked (§64)

- **OpenAI Responses `web_search`**: tool object with `filters.allowed_domains`,
  `user_location` (approximate: city, country ISO code, region, time zone),
  `external_web_access`; `tool_choice: "required"`; sources through
  `include: ["web_search_call.action.sources"]`; `url_citation`
  annotations. Checked against the official `openai` SDK type definitions
  (7.23.0) because platform.openai.com is not reachable from this build
  environment.
- **Anthropic web search / web fetch**: `web_search_20260209` and
  `web_fetch_20260209` (dynamic filtering) for Claude 4.6+ and the 5-series,
  `web_search_20250305` for older models; `allowed_domains`,
  `user_location`, `max_uses`; `web_search_tool_result` content or an error
  object with `error_code`; `pause_turn` continuation (platform.claude.com).
- **Unsloth Studio** (published package 2026.9.11, Studio backend source):
  the OpenAI-compatible `/v1/chat/completions` accepts `enable_tools`,
  `enabled_tools` (its own web tool is `web_search`; leaving the list out
  enables every Studio tool, including code execution), `permission_mode`;
  with `X-Unsloth-Events: 1` it streams `tool_start` / `tool_end` frames.
  No Unsloth code is copied (AGPL, §16): ReMa only sends documented request
  fields.

## 7. Limitations

- The no-key hosts (job boards, ATS APIs, Wikidata) are blocked from this
  build environment; they were exercised through local stand-ins that serve
  their documented response formats. The health check (**Check now**) must
  be run once on a normal network before release.
- The Bundesagentur für Arbeit job search API was not added: its only
  documentation is community-maintained and it relies on a shared client
  id; it can be added once its terms for third-party apps are confirmed.
  GDELT was not added (not a job or company-profile source).
- Contract and freelance requests are recognised; the employment-type filter
  is not applied to sources that do not state it.
- Codex (ChatGPT sign-in) has no domain filter; career sites are guidance in
  its brief.
