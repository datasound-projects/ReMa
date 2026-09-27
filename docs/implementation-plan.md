# Implementation plan — Run History, Career Search, Network Connect and Business

The plan and the living checklist for three specifications, implemented one
after another. Each requirement row names its specification section, where
it lives, its status and the evidence (or the blocker). Status values:
**Not started**, **In progress**, **Implemented** (code in place, not yet
verified), **Verified** (tests or an in-app run show it works), **Blocked**
(needs something outside this repository; the blocker is named).

The specification files are kept unchanged outside the repository:

| Short name | Exact specification filename |
|---|---|
| RH | `7fbae61e-ReMa_Scheduled_Task_Run_History_Implementation_1.md` |
| CS | `2968f2d9-ReMa_Always_On_Career_Search_Infrastructure.md` |
| NC | `ae663aa0-ReMa_Network_Connect_and_Business_Implementation_Guide.md` (Network Connect §1–§63 and **Appendix B — ReMa Business** B0–B40) |

## 1. Order

| Order | Exact specification filename | Scope | Why this position | Dependencies | Completion gate |
|---|---|---|---|---|---|
| 1 | `7fbae61e-ReMa_Scheduled_Task_Run_History_Implementation_1.md` | Persistent per-run history for every scheduled task: queued/running/succeeded/failed/cancelled runs with configuration and context snapshots, progress events, outputs by reference, safe errors, crash reconciliation, paginated history, the "Scheduled / Task" detail view, live updates; Job Mail & Interview Sync on the same system. | Everything else writes into it: CS §39 requires search metadata in run history, NC §42 and B26 require scheduled research to record runs. It changes the scheduler's execution boundary, which the later phases reuse unchanged. | The existing scheduler (`services/scheduler.rs`), `task_executions` (migrations 0001, 0002, 0004), Job Mail & Interview Sync (`services/mail_monitor.rs`, `jobs/`), job retrieval (`retrieval/`). | RH §53 tests pass; RH §54 criteria 1–28 checked; fmt, clippy `-D warnings`, `cargo test`, lint, typecheck, Vitest, build clean; in-app run of RH §55 step 20 (scheduled run, Run now, several runs, failure, edit, restart, built-in automation) in light and dark. |
| 2 | `2968f2d9-ReMa_Always_On_Career_Search_Infrastructure.md` | One provider-independent CareerSearch layer (router, requirement classes, scopes, source registry, provider-native search with domain filters and verification, Codex, Unsloth tools, no-key fallback chain, health and circuit breakers, normalization, freshness, salary status, evidence), used by Chat, Scheduled Tasks, Agents and the Jobs MCP; no "configure a search service" failure; Settings shows "Career Search — Automatic" with the external services as optional. | Network Connect and Business research (NC §5, §13–§17, §33–§37, B6, B8, B12) are built on this search layer; it needs Phase 1 to record search metadata per run (CS §39). | Phase 1 run context; `retrieval/`, `rema_mcp/` (engine, registry, safe fetcher, ATS adapters), `llm/` adapters (OpenAI Responses, Anthropic, Codex, Gemini, OpenAI-compatible), `services/chat.rs`, `services/websearch.rs`. | CS §61 tests pass; CS §62 criteria 1–30 checked; all checks clean; Phase 1 tests still pass; the reference request "Find me most recent AI jobs in Vienna Austria with salary starting from 85k a year." runs in Chat and as a Scheduled Task without a search key (live providers where reachable, local stand-ins otherwise, stated as such). |
| 3 | `ae663aa0-ReMa_Network_Connect_and_Business_Implementation_Guide.md` | Network Connect: LinkedIn and XING connectors on the shared connector infrastructure, capability registry, provider data policy, research service and planner, entities, evidence, confidence, UI, chat tools, scheduled research. Appendix B Business: Business Profile and offers, Find Clients, Find Contract Work, Business Pipeline, Go-to-Market Studio, tools, scheduling. | Needs CareerSearch (Phase 2) for every public research path and run history (Phase 1) for scheduled research; its connectors reuse the connectors layer. | Phases 1 and 2; `connectors/` (OAuth, token store, registry), `rema_mcp/fetch.rs` (hardened in Phase 2), Profile context, chat tool loop and approvals. | NC §60 and B34 tests pass; NC §61 and B36 checklists checked; all checks clean; Phases 1–2 regression tests pass; B35 manual scenarios run where reachable; B37 completion report written; provider blockers recorded. |
| Final | — | Integration verification across all three phases. | — | Phases 1–3. | Shared search, connectors, model/tool orchestration, scheduling, run history, persistence and navigation work together; one migration chain; no duplicate services or divergent models; full test suites and build; in-app end-to-end run; everything committed and pushed. |

Shared components have one owning phase:

| Component | Owner | Used by |
|---|---|---|
| Run record, snapshots, progress events, outputs, run context (`services/runs.rs`, migration 0010) | Phase 1 | Phase 2 fills the search part of the run context; Phase 3 scheduled research records runs |
| CareerSearch router, requirement classes, scopes, evidence model, company domain resolution, people/company search primitives, health | Phase 2 | Phase 3 research services |
| Hardened public fetcher (`rema_mcp/fetch.rs`: SSRF rules, redirects, limits, robots) | Phase 2 (extended, not duplicated) | Phase 3 website ingestion (B4, B30) |
| Network and Business models, capability registry, provider data policy, new task kinds, new connectors | Phase 3 | — |

## 2. Baseline (before any change)

- Branch `claude/rema-foundation-setup-6gmim3`, HEAD `671e287`, pushed, working tree clean.
- `cargo test --locked`: 468 passed, 0 failed, 3 ignored (explicit-only tests: bindings export, a measurement, a live Codex check).
- `pnpm test`: 87 passed (19 files). `cargo fmt --check`, `cargo clippy --all-targets --locked -- -D warnings`, `pnpm lint`, `pnpm typecheck`, `pnpm build`: clean at `671e287` (see [applications/validation.md](applications/validation.md)).
- Pre-existing failures: none. CI for `671e287` was still running when this plan was written.

## 3. Conflicts and how they are resolved

| Sections | Conflict | Resolution (no change to intended behavior, security or scope) |
|---|---|---|
| CS §13, §40, §59 vs NC B6 | CS: a missing search service must never be a user-facing configuration error. B6: "do not ship a decorative 'Automatic' label over an absent backend." | Both hold: the configuration errors are removed and every route is real (provider-native search, the Jobs MCP adapters, documented no-key public APIs). When every route fails, the run reports the actual cause (unreachable, rate limited, provider refused) without asking for a key; the health test (CS §50) shows which routes work. |
| CS §17–§18 ("no-key public search mechanism") vs NC B4, B30 ("never bypass … robots/access restrictions"), CS §2 (not a general browser), CS §44 | Free search-engine result pages are commonly closed to automated clients by robots rules or terms. | No-key routes are limited to documented public interfaces whose terms allow programmatic use (public ATS job-board APIs, public job-board APIs, Wikidata for company domains, company career pages within robots rules). Each route's basis is recorded in the source registry (as the Jobs MCP registry already does). Search-engine result pages are not scraped. |
| CS §3 Tier 1 (LinkedIn Jobs, XING Jobs, Indeed, StepStone …) vs NC §44 and the Jobs MCP registry | The tier lists sites whose terms were not reviewed for automated reading. | They stay discovery links (the registry's existing decision); results found through provider-native search can cite them; ReMa does not fetch them. |
| RH §26 ("every scheduler firing must produce exactly one run") vs the scheduler skipping an occurrence while the previous run is still busy | A skipped firing produced no record. | The firing is recorded as one run with status Cancelled and "Skipped: the previous run was still in progress." No scheduling change. |
| RH §27 (retries) | The scheduler has no run-level retries. | Nothing to redesign; transient-request retries inside a run stay internal and appear as activity where they matter. |
| NC §8 | LinkedIn's native-app PKCE flow must be enabled for the app by LinkedIn. | Implemented against the documented flow and tested with a local stand-in; live sign-in is a release prerequisite (below). |
| NC §11 | XING offers "Login with XING" only as a website plugin bound to a domain, and no new XING API applications can be registered. | The XING connector reports its real capability state (not available for desktop sign-in); public discovery uses permitted sources only (NC §1 "build around capabilities"). |

## 4. External blockers and release prerequisites

| Item | Needed from | Affects |
|---|---|---|
| LinkedIn app with "Sign In with LinkedIn using OpenID Connect" and native PKCE enabled; client id for the build | Publisher / LinkedIn | NC §8 live sign-in |
| LinkedIn approval for any member-network scope (e.g. `r_1st_connections`, partner-only) | LinkedIn partner programme | NC §9, §22 (first-degree data); without it the capability stays "Not available" |
| XING desktop sign-in and API access (no new applications) | XING | NC §11–§12 |
| Live verification of official documentation hosts that are blocked here (platform.openai.com, learn.microsoft.com, dev.xing.com) | Re-check before release | CS §64, NC §63, B40 (verified here through the official OpenAI SDK type definitions, platform.claude.com, Unsloth's published package and search summaries of the official pages) |
| Live no-key routes (job boards, ATS APIs, Wikidata) are unreachable from this build environment | Re-run the health test on a normal network | CS §50, §60 |
| Privacy and legal review of the Business defaults (retention of prospect data, lawful basis for public professional data about people, national rules on unsolicited electronic marketing, platform terms) | The publisher's privacy/legal review | B29, B36 (last item), B40 |

## 5. Phase 1 — Run History (RH)

Design: the existing `task_executions` table stays the run table (no parallel
system). Migration 0010 rebuilds it with the new statuses and snapshot
columns and adds `task_run_events` (progress) and `task_run_outputs`
(outputs by reference), both deleted with their run. `services/runs.rs`
holds the lifecycle; the scheduler, Run now and Job Mail & Interview Sync all
go through it.

| Req | Requirement | Planned location | Status | Evidence / blocker |
|---|---|---|---|---|
| RH §1, §49 | Keep the overview table and actions | `src/pages/ScheduledTasksPage.tsx` | Verified | Overview unchanged; Vitest page tests; E2E |
| RH §2 | Open a task by its row or name; `…` menu keeps the actions | `ScheduledTasksPage.tsx` | Verified | Row click and History open the runs; Vitest; E2E |
| RH §3 | Detail view "Scheduled / <Task Name>" | `src/app/navigation.ts` (`tasks` view with `taskId`, `runId`), `src/components/tasks/TaskRunsView.tsx` | Verified | `View.tasks{taskId, runId}`; breadcrumb; Vitest; E2E |
| RH §4 | Layout: selected run main area, run history panel on the right | `TaskRunsView.tsx`, `src/styles/taskRuns.css` | Verified | `TaskRunsView` two panes, independent scrolling; E2E screenshots |
| RH §5, §31 | Run list newest first, compact, selection obvious | `src/components/tasks/RunList.tsx` | Verified | `RunList`; newest first, `aria-current`; Vitest; E2E |
| RH §6 | Trigger recorded; Manual label | `task_executions.trigger`; `RunList.tsx` | Verified | Manual tag and header; Rust and Vitest; E2E |
| RH §7 | Queued, Running, Succeeded, Failed, Cancelled | migration 0010; `ExecutionStatus` | Verified | Migration 0010; `ExecutionStatus`; tests for each state |
| RH §8 | Header: status, trigger, started, finished/failed, duration or elapsed | `RunHeader` in `TaskRunsView.tsx` | Verified | `RunView` header (elapsed live); E2E |
| RH §9, §13 | Every run keeps its own final output (Markdown) | `task_executions.result`, `report` | Verified | `every_firing_and_every_run_now_is_one_run_with_its_own_result`; E2E |
| RH §10, §11, §20, §21, §37 | Snapshot of name, prompt, model, schedule, Profile, kind at execution time | migration 0010 columns; `services/runs.rs::create` | Verified | `runs_keep_the_task_as_it_was_through_edits_pause_and_resume`; E2E edit |
| RH §12, §23 | No secrets in runs; error category, safe message, time | `RunContext` (names only); errors through `llm::http::scrub`; security test | Verified | `safe_messages_carry_no_credentials`; Job Mail run scan; failed-run test |
| RH §14, §15, §39, §40 | 0..N outputs by reference, Outputs section, open an output | `task_run_outputs`; `RunInspector` | Verified | Outputs (run report, Analytics job search, application); tests; E2E |
| RH §16, §17, §18, §30 | Real progress events (no reasoning text) | `task_run_events`; recorder used by scheduler, retrieval and `jobs::run` | Verified | Stages for prompt, job search and Job Mail tasks; tests; E2E |
| RH §19 | Context section (Profile, connectors, web search) | `task_executions.context` | Verified | `RunContext`; E2E (connectors, Profile, web search) |
| RH §22 | Failure view with guidance | `error_category` → guidance in `TaskRunsView.tsx` | Verified | `failureGuidance`; Vitest failure view; E2E interrupted run |
| RH §24, §47 | Running run appears at once and updates live | `TaskRunChanged` event; `useTaskRuns` | Verified | `TaskRunChanged`; Vitest live update; E2E running run and scheduled firing |
| RH §25, §42 | Run created (queued) before execution, at the scheduler boundary | `scheduler::spawn_run` / `execute` | Verified | `spawn_run` creates the queued run first; `a_run_is_visible_while_running…` |
| RH §26 | One firing or Run now = exactly one run | `spawn_run`; skipped firing recorded | Verified | One run per firing / Run now; skipped firing recorded; tests |
| RH §27 | Retry semantics | — | Verified | No run-level retries exist; a second search attempt is shown in the stage label |
| RH §28 | Run now creates a manual run and opens it | `run_task_now` returns the run id; UI navigates | Verified | `run_task_now` returns the id; Vitest "opens the new run after Run now"; E2E |
| RH §29, §51 | Job Mail & Interview Sync on the same system, with progress and outputs | `mail_monitor.rs`, `jobs/mod.rs` | Verified | `job_mail_sync_records_a_normal_run…`; E2E |
| RH §32 | Paginated history, independent scroll, newest first | `list_task_runs(task_id, before, limit)`; incremental loading | Verified | 30 per page; Vitest "loads older runs only when asked" |
| RH §33, §35, §36 | No retention cut; pause, resume and edits keep history | `services/tasks.rs`; tests | Verified | No retention cut; edit/pause/resume test |
| RH §34 | Deleting a task deletes its runs, events and outputs | FK `ON DELETE CASCADE` | Verified | FK cascade; migration test |
| RH §38 | Safe Markdown, tables, links, code | existing `Markdown` component | Verified | Existing safe `Markdown` and `JobReport` |
| RH §41 | Backend interfaces (`list_task_runs`, `get_task_run`, create/running/progress/output/complete/fail) | `services/runs.rs`, `db/runs.rs`, `commands/tasks.rs` | Verified | `db/runs.rs`, `services/runs.rs`, commands |
| RH §43, §44 | Stale runs reconciled on start ("Execution interrupted before completion."); history survives restart | `mark_interrupted_executions` at startup | Verified | `mark_interrupted`; restart test; E2E kill and restart |
| RH §45, §46 | UTC milliseconds stored; local "Today at …"; scheduled vs started vs finished kept | `src/lib/format.ts` | Verified | UTC ms stored; `formatWhen`; "Scheduled for" when late |
| RH §48 | Empty state with Run now | `TaskRunsView.tsx` | Verified | Empty state with Run now; Vitest; E2E |
| RH §52 | Nothing outside the scope | — | Verified | Nothing outside the scope added |
| RH §53 | Required tests | `db/runs.rs`, `services/runs.rs`, `services/scheduler.rs`, `db/mod.rs`, Vitest | Verified | See [run-history/validation.md](run-history/validation.md) |
| RH §54 | Acceptance criteria 1–28 | [run-history/validation.md](run-history/validation.md) | Verified | All 28 verified: [run-history/implementation.md](run-history/implementation.md) §6 |

## 6. Phase 2 — Always-On Career Search (CS)

Layout: `src-tauri/src/career_search/` (requirement classes, scopes, plan,
registry, router, no-key job sources, company research, health, status),
built on `retrieval/` and `rema_mcp/` rather than beside them. Record:
[career-search/implementation.md](career-search/implementation.md);
validation: [career-search/validation.md](career-search/validation.md).

| Req | Requirement | Location | Status | Evidence / blocker |
|---|---|---|---|---|
| CS §1, §37 | A search path always exists, the same in Chat, Tasks, Agents, Jobs MCP | `career_search/router.rs`; `services/chat.rs`, `services/scheduler.rs`, `rema_mcp/engine.rs` | Verified | E2E 3–12; chat, scheduler and router tests |
| CS §2 | Professional scope only, not a general browser | `requirement.rs` scopes, `registry.rs` | Verified | Domain filters from the career registry (E2E 4–5, 10); `scopes_choose_their_sources` |
| CS §3, §19, §23 | Source hierarchy; Jobs MCP first; one registry | `registry.rs`; `rema_mcp/sources.rs` (read policy) | Verified | "ReMa Jobs" first in every route; `job_searches_stay_on_job_sources_of_the_region`, `urls_map_to_their_source` |
| CS §4, §5, §6 | One interface, one router, automatic selection | `router.rs` (`search_jobs`, `research`) | Verified | E2E 3–6: four engines, no setting changed |
| CS §7–§9, §31 | OpenAI native search, domain filters, source validation | `llm/openai_responses.rs`, `retrieval/native.rs` | Verified | E2E 5; `builds_stateless_requests_with_web_search`, `counts_only_searches_that_ran`, `research_findings_need_a_reported_page` |
| CS §10 | Codex | `retrieval/native.rs` (ChatGPT web search) | Verified | Engine and brief with the sites as guidance; findings marked unchecked (unit tests); no live Codex run here |
| CS §11, §12, §32 | Anthropic native search, verification, sources | `llm/anthropic.rs`, `retrieval/native.rs` | Verified | E2E 4, 7, 10; `picks_the_web_tool_versions_each_model_has`, fallback tests |
| CS §13, §40, §59 | No configuration error; the dependency removed | `retrieval/mod.rs`, `services/chat.rs`, `rema_mcp/engine.rs`, `retrieval/render.rs` | Verified | E2E 8; `every_route_failing_is_an_honest_operational_error` |
| CS §14–§16 | Local models get a search tool; Unsloth web search on automatically; no AGPL code | `retrieval/tools.rs`, `llm/openai.rs`, `services/providers.rs` | Verified | E2E 6; `unsloth_runs_only_its_web_search_and_reports_it`, `local_models_get_remas_web_tools_for_other_questions` |
| CS §17, §18 | No-key fallback, fault tolerant | `career_search/jobs.rs`, `company.rs`, adapters | Verified | E2E 3, 8, 9; `one_failing_source_does_not_stop_the_others`, `a_source_that_could_not_be_searched_is_named_in_the_answer` |
| CS §20, §21 | Requirement classes; current-data guard | `requirement.rs`; chat routing | Verified | `current_career_questions_require_search`, `static_questions_need_no_search`; E2E 9 |
| CS §22 | Scopes | `requirement.rs` | Verified | Search scope in run context (E2E 11, 12) |
| CS §24, §25 | Company domains; location-aware search | `company.rs` (Wikidata P856), `plan.rs` (place, ISO code, time zone) | Verified | `user_location` Vienna (E2E 4–5); `plans_the_screenshot_request` |
| CS §26 | Query planner | `plan.rs` | Verified | Roles and variants, place, salary, recency, companies, people; plan tests |
| CS §27–§30 | Salary, freshness, normalization, evidence | `retrieval/listings.rs`, `router.rs` | Verified | Salary rules and notes (E2E 3); `applies_the_date_window_and_the_salary_floor`; snapshot time shown (`a_search_repeated_within_minutes…`) |
| CS §33–§36 | Dedupe, failure isolation, retries, circuit breakers | `router.rs` merge, `health.rs` | Verified | E2E 4 (one row for two sources); `repeated_failures_rest_a_source…`, `a_rate_limited_search_is_retried_once` |
| CS §38, §39 | Scheduled tasks; search metadata in run history | `services/scheduler.rs`; `RunContext.search_scopes`, `sources_consulted` | Verified | E2E 11, 12 (Manual and Scheduled runs) |
| CS §41, §42 | Settings "Career Search — Automatic"; external services optional | `CareerSearchSection.tsx`, `AdditionalSearchService.tsx` | Verified | E2E 1, 2, 6; Vitest |
| CS §43, §44 | No external browser window; no hidden GUI automation | — | Verified | E2E 13; only API and permitted-page requests |
| CS §45–§48 | Prompt-injection protection; source fetching; JS-heavy pages; JobPosting data | `research.rs` context block, `rema_mcp/fetch.rs`, `analytics/page.rs` | Verified | E2E 9 (no contacts, data block, no tools); fetch and JSON-LD tests |
| CS §49–§51 | Observability; health test; start-up capability detection | `career_search/{mod,status}.rs` | Verified | Recent searches (E2E 12), Check now (E2E 2), Unsloth detected at save and start-up (E2E 6) |
| CS §52–§55 | Routing examples (OpenAI, Claude, Unsloth, other local) | router | Verified | E2E 3–6 |
| CS §56–§58 | Quality guard; no fake current results; progress UI | chat, render, `WebActivity.tsx` | Verified | "Searched career sources · …" (E2E 3); nothing from model memory (tests) |
| CS §60 | Realistic reliability | health, fallback | Verified | E2E 7, 8; live hosts: release prerequisite (§4) |
| CS §61 | Required tests | | Verified | [career-search/validation.md](career-search/validation.md) §1 |
| CS §62 | Acceptance criteria 1–30 | | Verified | [career-search/validation.md](career-search/validation.md) §4 |
| CS §64 | Official documentation verification | [career-search/implementation.md](career-search/implementation.md) §6 | Verified | OpenAI SDK types 7.23.0, platform.claude.com, Unsloth 2026.9.11 package; OpenAI's site to re-check on a normal network |

## 7. Phase 3 — Network Connect (NC §1–§63) and Business (Appendix B)

Layout: `src-tauri/src/network/` and `src-tauri/src/business/`, migrations
0011 and 0012, pages `NetworkConnectPage.tsx` and `BusinessPage.tsx`.
Records: [network-connect/implementation.md](network-connect/implementation.md),
[network-connect/validation.md](network-connect/validation.md),
[business/implementation.md](business/implementation.md),
[business/validation.md](business/validation.md).

| Req | Requirement | Location | Status | Evidence / blocker |
|---|---|---|---|---|
| NC §1, §6, §51, §52 | Capability registry and honest capability states | `network/capabilities.rs`; capability cards | Verified | Capability tests; NC validation 2.1, 2.3, 2.9 |
| NC §2, §3, B1 | Navigation (Network Connect, Business) and purpose | `src/app/pages.ts`, sidebar | Verified | Both pages in the app |
| NC §4, §19, §35, §36 | Professional graph, people schema, entity resolution, confidence | `network/model.rs`, `resolve.rs`, `evidence.rs` | Verified | Resolution and confidence tests (subsidiaries kept apart); NC validation 2.5 |
| NC §5, §33, §34, §37 | One research layer on CareerSearch; source strategy; evidence; freshness | `network/service.rs`, `companies.rs`, `people.rs` | Verified | Research tests; retrieval times on every claim (NC 2.6) |
| NC §7, §8, §53 | Account connection UX; LinkedIn OpenID Connect with PKCE; shared connectors | `connectors/linkedin.rs`, Settings → Connectors | Verified (stand-in) | NC 2.3, 2.12; live sign-in needs LinkedIn's native-PKCE enablement and a client id (§4) |
| NC §9, §10, §22, §23 | LinkedIn network and people lookup only as permitted | `network/capabilities.rs`, `relationships.rs` | Verified (stand-in) | NC 2.9–2.11; live first-degree access needs LinkedIn's approval of `r_1st_connections` (§4) |
| NC §11, §12 | XING authentication and retention | `connectors/xing.rs`, `network/policy.rs` | Verified | Shown unavailable with its reason; member data denied for every purpose (tests) |
| NC §13, §14 | Public discovery separate; Jobs MCP stays the job layer | `network/companies.rs` | Verified | `a_network_request_becomes_a_job_search`, `jobs_group_into_their_employers` |
| NC §15–§18, §24–§27 | Company, hiring activity, job → people, hiring manager vs contact, recruiters, technology, Profile-aware research | `network/people.rs`, `planner.rs` | Verified | People tests; NC 2.5, 2.10b |
| NC §20, §48, §49 | Multi-hop planner; query examples | `network/planner.rs` | Verified | Planner tests; example requests on the page |
| NC §28–§31 | UI, result modes, drill-down, direct links | `NetworkConnectPage.tsx` | Verified | Vitest; NC 2.5–2.7 |
| NC §32, §47 | Chat integration; tool contracts | `network/tools.rs`, `services/chat.rs` | Verified | Chat tests; NC 2.11 |
| NC §38, §39, §40 | Privacy boundary; provider data policy; temporary vs persistent | `network/policy.rs` | Verified | Policy tests; SQLite checks (NC 2.11, 2.12) |
| NC §41, §42 | Networking CRM; tracking companies (scheduled) | `network/`, `services/scheduler.rs` | Verified | No CRM without a permitted source (§41); **Track this company…** (NC 2.8) |
| NC §43–§46 | No outreach, no authenticated automation, security, prompt injection | tools, fetcher, render | Verified | `no_authenticated_scraping_path_exists`; injection audit (NC 2.11) |
| NC §50 | Result quality rules | `network/service.rs`, `render.rs` | Verified | Nothing invented; unknowns stated (tests, NC 2.5) |
| NC §54–§59 | Backend architecture, interfaces, data flows | `network/` | Verified | [network-connect/implementation.md](network-connect/implementation.md) |
| NC §60, §61 | Tests; acceptance criteria | `docs/network-connect/validation.md` | Verified | §1 and §4 of that report |
| NC §62, §63 | Official documentation | NC record §5 | Verified (search excerpts) | learn.microsoft.com and dev.xing.com unreachable here; re-check before release (§4) |
| B0–B2 | Additive scope; Business navigation; shared architecture, separate commercial state | `business/`, migration 0012 | Verified | Business validation §4 |
| B3–B5 | Business Profile, offers, website → reviewed offer, context versioning | `business/offers.rs`, `ingest.rs`, `store.rs` | Verified | Offer tests; B35 scenario 2 |
| B6, B7 | Search infrastructure and scope; location and eligibility | career search, `business/locations.rs` | Verified | Location tests |
| B8–B11 | Find Clients: workflow, ICP, reproducible fit, buyers | `business/clients.rs`, `fit.rs` | Verified | Client and fit tests; B35 scenarios 1, 2, 5 |
| B12–B14 | Find Contract Work: classification, filters, rate logic, results | `business/contracts.rs` | Verified | Contract tests; B35 scenario 3 |
| B15, B16 | Business Pipeline; opportunity identity | `business/pipeline.rs` | Verified | Pipeline tests; B35 scenario 6 |
| B17–B23 | Go-to-Market Studio: segments, competitors, channels, target accounts, drafts, experiments, metrics | `business/gtm.rs`, `experiments.rs` | Verified | GTM and experiment tests; B35 scenario 4 |
| B24, B25 | Domain model and storage; tool contracts and permissions | migration 0012, `business/model.rs`, `tools.rs` | Verified | `there_is_no_sending_tool_and_changes_need_approval` |
| B26 | Chat, scheduling and run history | `services/chat.rs`, `services/scheduler.rs` | Verified | Chat and scheduler tests; scheduled Business task in the app |
| B27 | UI states and progressive results | Business pages | Verified | Vitest; unfinished-run notice (B35 scenario 6) |
| B28–B31 | Provider purpose; privacy and retention; fetching and injection; reliability | `network/policy.rs`, fetcher, `pipeline.rs` | Verified | Business validation §1; scenario 7 |
| B32, B33 | Boundaries; sequence | — | Verified | No sending, payment, ads or proposal submission |
| B34, B35, B36 | Tests; manual scenarios; final checklist | `docs/business/validation.md` | Verified | §1, §2, §4 of that report (local stand-ins) |
| B37 | Completion report | `docs/business/implementation.md` | Verified | Written; legal review open (B40, §4) |

## 8. Phase reports

Each phase adds its report here when its gate passes (what was implemented,
files and migrations, acceptance verified, commands and results, limitations).

### Phase 1 — Run History: gate passed

- **Implemented**: one run lifecycle for every execution (scheduled
  firings, Run now, Job Mail & Interview Sync): the run is created queued
  with a snapshot of the task, then running, with stages, context and
  outputs recorded as they happen, and ends succeeded, failed (safe message
  and category) or cancelled; a busy firing is recorded as skipped; runs a
  previous session left open are marked interrupted at start-up. The
  "Scheduled / <Task>" view: run list (paged, live), selected run (result,
  live progress with Stop, or error with guidance) and an inspector with
  Progress, Context, Outputs and Task configuration.
- **Files and migrations**: migration `0010_run_history.sql`;
  `db/runs.rs`, `services/runs.rs`; the scheduler, Job Mail pipeline,
  commands, models and events; `TaskRunsView.tsx`, `useTaskRuns.ts`,
  `taskRuns.ts`, `Collapsible.tsx`; the E2E stand-in's mail, delay and
  event-id changes. Record: [run-history/implementation.md](run-history/implementation.md).
- **Acceptance**: criteria 1–28 verified (table in the record).
- **Commands**: fmt and clippy clean; `cargo test` 480 passed (3 ignored,
  as before); lint and typecheck clean; Vitest 98 passed; build succeeds.
  In-app run: [run-history/validation.md](run-history/validation.md).
- **Limitations**: providers were local stand-ins; the longest in-app
  history was eight runs (paging is covered by tests).

### Phase 2 — Always-On Career Search: gate passed

- **Implemented**: one career-search layer (`career_search/`) used by Chat,
  Scheduled Tasks (scheduled and Run now), Agents and the Jobs MCP. Job
  requests run ReMa Jobs (the Jobs MCP over no-key sources: employers' ATS
  boards, Arbeitnow, The Muse, Remotive, Hacker News "Who is hiring?") and
  the model's own search (OpenAI Responses `web_search` with domain filter,
  location and `tool_choice: required`; Anthropic `web_search`/`web_fetch`
  with domain filter and location; Codex; Gemini; Unsloth Studio's
  `web_search` only) at once, verify what the model reports, merge
  duplicates (the employer's own posting kept), apply the date and salary
  rules and show every row with its source. Research requests (companies,
  people, market) search Wikidata, Wikipedia and the company's own pages
  plus the model's search, then answer from a delimited source block with
  a Sources list. The "No search service is set up" failure is gone; when
  every route fails the answer says what happened. Settings shows **Career
  Search — Automatic** with Check now, Recent searches and the optional
  service under Advanced.
- **Files**: `career_search/` (new), `rema_mcp/adapters/{boards,
  smartrecruiters,workable,recruitee}.rs` (new) and the list endpoints of
  the existing ATS adapters, `rema_mcp/{engine,sources,fetch,host}.rs`,
  `retrieval/{mod,native,listings,render,tools,intent}.rs`, `llm/{mod,
  openai,openai_responses,anthropic}.rs`, `services/{chat,scheduler,
  providers}.rs`, `analytics/page.rs` (character sets),
  `CareerSearchSection.tsx`, `AdditionalSearchService.tsx`,
  `WebActivity.tsx`, `taskRuns.ts`, `TaskRunsView.tsx`; the E2E stand-in's
  career sources and model searches. No migration (run context fields are
  optional JSON).
- **Acceptance**: criteria 1–30 verified (table in the validation record).
- **Commands**: fmt and clippy clean; `cargo test` 531 passed (3 ignored,
  as before); lint and typecheck clean; Vitest 102 passed; build succeeds.
  In-app runs with OpenAI, Anthropic, Unsloth and a plain local model
  stand-in, no search service configured, in Chat and as a scheduled task.
- **Limitations**: the no-key hosts and the providers are unreachable from
  this environment (stand-ins used; Check now on a normal network before
  release); Bundesagentur für Arbeit not added (terms for third-party use
  unconfirmed); Codex has no domain filter (guidance in its brief).

### Phase 3 — Network Connect and Business: gate passed

- **Implemented**: Network Connect researches companies, their current
  jobs and the relevant people. It works from public, permitted sources:
  ReMa's job layer, Wikidata, the companies' own pages, and pages the
  model's search reported.
  - It adds the user's first-degree LinkedIn connections only when
    LinkedIn granted `r_1st_connections`. They are shown for the session
    only, never stored and never sent to a model; a chat and its model get
    only their count.
  - Every capability comes from the granted scopes, and a missing permission
    is said, never reported as "no connections".
  - It is available on its page, in Chat (and as tools), and as a daily
    tracking task.

  ReMa Business adds:
  - reviewed, versioned offers (manual, from a URL or from a document);
  - Find Clients with a reproducible fit measure;
  - Find Contract Work with strict terms;
  - one commercial Pipeline, separate from Applications, with Do not
    contact and deletion;
  - Go-to-Market Studio: hypotheses, positioning, local drafts,
    experiments, metrics from recorded activity;
  - Chat and scheduled research.

  Nothing sends, pays or submits anything. Records:
  [network-connect/implementation.md](network-connect/implementation.md),
  [business/implementation.md](business/implementation.md).
- **Files and migrations**: migrations `0011_network_connect.sql` (LinkedIn
  and XING in the connector tables) and `0012_business.sql` (Business
  tables); `network/`, `business/`, `connectors/{linkedin,xing}.rs`,
  `commands/{network,business}.rs`, `services/{chat,scheduler}.rs`,
  `NetworkConnectPage.tsx`, `BusinessPage.tsx`, `components/business/`,
  Settings → Connectors (professional networks); the E2E stand-in's
  LinkedIn, company sites, product site and contract listings.
- **Acceptance**: NC §61 criteria 1–30
  ([network-connect/validation.md](network-connect/validation.md) §4) and
  the B36 checklist ([business/validation.md](business/validation.md) §4)
  are verified with local stand-ins. The B35 scenarios 1–7 were run in the
  app, and 7 was re-run on the final build. Problems found in the app were
  fixed and are listed in both reports.
- **Commands** (final run, commit `2c3697f` plus these documents):
  - `cargo fmt --check` and `cargo clippy --all-targets --locked -- -D
    warnings` are clean.
  - `cargo test --locked`: 650 passed, 0 failed, 3 ignored (the
    explicit-only bindings export, measurement and live Codex tests).
  - `pnpm lint` and `pnpm typecheck` are clean. `pnpm test` (Vitest): 116
    passed in 23 files. `pnpm build` succeeds.
- **Limitations and external blockers**:
  - Providers were exercised against local stand-ins only.
  - A live LinkedIn sign-in needs a LinkedIn app with native PKCE enabled
    and its client ID (§4). Live connection lists need LinkedIn's approval
    of `r_1st_connections`. XING offers no integration.
  - The official pages on learn.microsoft.com and dev.xing.com were read
    through search excerpts; re-check them on a normal network.
  - The Business defaults need the privacy and legal review listed in §4.
  - Prompt injection is handled by structure (delimited data, no tools on
    answers, exact quotes, review), not by a classifier.

### Final integration verification

- **One system**: Chat, Scheduled Tasks, the Network Connect page and
  Business use one career-search router, one Jobs MCP, one connector
  registry and credential store, one scheduler with Phase 1 run history,
  and one migration chain (0001–0012). There are no duplicate search,
  credential or scheduling services. Chat tells job, network and business
  requests apart (tests in `business/tools.rs`, `services/chat.rs`).
- **Automated**: the full Rust and Vitest suites and the production build
  pass (numbers above). They include every Phase 1 and Phase 2 test.
- **In the app** (debug build, final code):
  - Phase 1: Run now on the scheduled Business task "Weekly AI automation
    prospects" produced a succeeded manual run with its two stages and
    result next to the scheduled run.
  - Phase 2: "Find senior AI engineering jobs in Vienna" in Chat listed six
    current postings with their sources (Arbeitnow, Greenhouse, The Muse,
    the employer's page) and "Not shown: 1 closed".
  - Phase 3: the Network Connect and Business runs in the two validation
    reports.
  - The Business and Network Connect pages were checked in both themes.
