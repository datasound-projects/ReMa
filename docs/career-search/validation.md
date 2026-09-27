# Always-On Career Search — validation report

What was run and what was observed. Environment: Linux container, Xvfb
display, D-Bus session with GNOME Keyring, Rust 1.98.1. External job hosts,
Wikidata and the model providers are not reachable from this environment, so
the in-app runs used the local stand-ins in `scripts/e2e/mock-providers.mjs`:
ReMa's no-key sources in their documented response formats
(`REMA_DEV_ATS_BASE`), a company website and posting pages
(`REMA_DEV_ALLOW_LOCAL_PAGES`), Anthropic Messages and OpenAI Responses
endpoints whose web search reports results (`REMA_ANTHROPIC_BASE_URL`,
`REMA_OPENAI_BASE_URL`), an Unsloth Studio server and a plain
OpenAI-compatible local server. The stand-ins log every model request with
its step, tools, domain filter, location, and whether credentials, contact
details or ReMa's source block were in it. Brave, Tavily and SearXNG were
never configured.

## 1. Automated checks

| Check | Result |
|---|---|
| `cargo fmt --check` | clean |
| `cargo clippy --all-targets --locked -- -D warnings` | clean |
| `cargo test --locked` | 531 passed, 3 ignored (the explicit-only tests, as before) |
| `pnpm lint`, `pnpm typecheck` | clean |
| `pnpm test` | 102 passed (21 files) |
| `pnpm build` | built |

The tests §61 asks for:

| §61 | Tests |
|---|---|
| Search requirement | `static_questions_need_no_search`, `current_career_questions_require_search` (jobs, company, people), `general_company_knowledge_is_optional`, `plans_the_screenshot_request`, `plans_people_and_company_research` |
| OpenAI | `builds_stateless_requests_with_web_search` (web search attached, `external_web_access`, `tool_choice: required` for the search step, `filters.allowed_domains`, `user_location`, sources included), `openai_uses_the_responses_api_with_web_search`, `reports_url_citations`, `counts_only_searches_that_ran`, `a_required_search_fails_rather_than_answering_without_it`, `a_model_that_does_not_search_falls_back_to_remas_job_sources` |
| Anthropic | `picks_the_web_tool_versions_each_model_has` (tool versions, `allowed_domains`, `user_location`), `anthropic_searches_the_web_and_resumes_a_paused_turn`, `reports_search_errors_that_arrive_with_a_200`, `provider_search_that_is_unavailable_falls_back_to_remas_sources` |
| Local / Unsloth | `unsloth_runs_only_its_web_search_and_reports_it` (only `web_search` enabled, `permission_mode: off`, tool frames → sources), `unsloths_own_search_failing_falls_back_to_remas_sources` |
| Other local models | `local_models_search_remas_job_sources_without_any_setup`, `local_models_get_remas_web_tools_for_other_questions` (`rema_career_search` exposed), `local_models_with_no_source_reachable_get_an_honest_failure` |
| Settings | `is_automatic_and_works_with_no_service_set_up`, `a_misconfigured_optional_service_never_blocks_search` (Brave chosen, no key), `the_screenshot_request_works_with_no_search_service_and_keeps_the_salary_rules` (no service); Vitest `CareerSearchSection` (automatic without a service, Check now, the optional service under Advanced) |
| Scheduled tasks | `scheduled_job_searches_use_the_career_router_with_no_search_service` (Run now and the scheduler share `execute`), `scheduled_research_searches_first_and_lists_its_sources` |
| Results | `the_employers_posting_wins_and_duplicates_merge`, `a_checked_duplicate_vouches_for_a_page_that_did_not_open`, `drops_closed_expired_and_unrelated_postings`, `applies_the_date_window_and_the_salary_floor`, `summaries_keep_the_salary_rules`, `writes_a_safe_source_backed_table` (source URL on every row), `a_model_that_does_not_search_gets_no_listings_through` (nothing from model memory), `a_search_repeated_within_minutes_says_when_its_sources_were_read` |
| Failure | `one_failing_source_does_not_stop_the_others` (and a resting source is not asked again), `a_source_that_could_not_be_searched_is_named_in_the_answer`, `provider_search_that_is_unavailable_falls_back_to_remas_sources`, `every_route_failing_is_an_honest_operational_error` (no "configure", "Settings", "API key", Brave, Tavily or SearXNG), `research_with_no_reachable_source_is_an_honest_failure`, `answers_without_web_access_say_so_and_ask_for_no_setup` |
| Security | `research_answers_from_company_sources_without_contacts_or_instructions` and `current_people_questions_search_first_and_answer_only_from_sources` (page text only inside `<career_sources>`, the answer request has no tools, contacts redacted), `hands_the_listings_to_the_model_as_data`, the screenshot test (no credential in any model request), `writes_a_safe_source_backed_table` (no HTML, links or table breaks from page text), `never_reaches_the_local_network`, `names_that_resolve_to_private_addresses_are_refused_at_connect_time`, `reads_the_declared_character_set` |

## 2. In-app end-to-end (debug build)

The data directory was the one left by the Run History phase (database
version 10, Gmail, Outlook and both calendars connected, a local
OpenAI-compatible model as the default). Brave, Tavily and SearXNG were not
configured at any point.

| # | Scenario | Observed |
|---|---|---|
| 1 | Settings → Career Search | "Automatic"; "Selected model: mock-classifier — ReMa searches for it"; ReMa job sources, Company and people research and Model web search with what each covers; Advanced (optional) holds the search service, set to "None (not needed)". |
| 2 | Check now | ReMa job sources "Answered (5 current jobs on the first page)", Company research "Answered.", Model web search "mock-classifier has no web search of its own; ReMa's career search tools are used instead." (Arbeitnow page 1 and Wikidata "Siemens" requested). |
| 3 | Chat, local model, "Find me most recent AI jobs in Vienna Austria with salary starting from 85k a year." | "Searched career sources · ReMa Jobs · 3 searches · 0 pages read · 4 postings": Senior AI Engineer (Donau Data, €95k / year, Arbeitnow), Machine Learning Engineer (Nordlicht AI, no salary), LLM Engineer (Wien Robotics, €95k–120k, Hacker News, "whether it is still open is not stated · salary period not stated"), Applied AI Engineer (Muse Robotics, The Muse). "Salary: 2 state a salary from 85,000/year · 2 do not state one (shown, not confirmed to meet the minimum). Not shown: 1 below the salary minimum." The Berlin posting and the Accountant were not shown. Then the model's assessment. |
| 4 | Same request, Claude Sonnet 5 (Anthropic API key) | Search request: `web_search_20260209` + `web_fetch_20260209`, 20 allowed domains, location Vienna, no credential in the body. Five postings from "ReMa Jobs + Anthropic web search (4 searches) · 2 pages read": the Prater AI posting page (JSON-LD, €88k–105k / year) added; the Wien Robotics vacancy shown once, with its Greenhouse address (the employer's posting) and "checked on Hacker News"; the expired AI Lead left out ("1 closed"). The answer request had no tools. |
| 5 | Same request, GPT-5 (OpenAI API key) | Responses request: `web_search` with 20 allowed domains, location Vienna, `tool_choice: required`, `include: web_search_call.action.sources`; same five postings, engine "ReMa Jobs + OpenAI web search". |
| 6 | Same request, Unsloth Studio model | Search request with `enable_tools: true`, `enabled_tools: ["web_search"]`, `permission_mode: "off"`, `X-Unsloth-Events: 1`; the answer request enabled no tools. Same five postings, engine "ReMa Jobs + Unsloth web search". Settings shows "with Unsloth web search" when it is the default model. |
| 7 | Claude with its web search unavailable | Four postings from ReMa Jobs and "Anthropic web search: the search failed (Search is unavailable right now). Used ReMa Jobs instead." The ReMa Jobs part came from the search the scheduled task had made five minutes earlier and said so ("retrieved … 08:45"). |
| 8 | Every no-key source answering 503 (after a restart, so nothing cached), local model, a new request | "ReMa could not retrieve live career sources for this request right now. No job listings are shown, because none could be verified. What happened: ReMa job sources: the source is temporarily unavailable (503)." Nothing about configuring a service. |
| 9 | Chat research, local model, "Who are the current recruiters at Nordlicht AI?" | Wikidata (the "river" namesake skipped), Wikipedia, the company's robots.txt, homepage (read once), team and careers pages, its Greenhouse board; the model request carried `<career_sources>`, no contact details, no tools; the answer ends with the numbered Sources list (official, reference). No "Analyze jobs" button under it. |
| 10 | Same research, Claude | Search step with a 5-domain filter and no ReMa context; the team page it reported merged with ReMa's own; the page it listed but never reported was dropped; "ReMa sources + Anthropic web search". |
| 11 | Scheduled task "Vienna AI jobs" (the reference request): Run now, its scheduled firing at 08:45, Run now again | Three runs (Manual, Scheduled, Manual), each succeeded with the four postings; stages "Searched with ReMa Jobs · 3 searches", "Checked 5 postings · 4 matched, 1 left out", "Assessed 4 listings"; Context: Search "Used · 3 searches", Search scope "Jobs", Sources consulted "ReMa Jobs, Arbeitnow, The Muse, Hacker News: Who is hiring?". |
| 12 | Scheduled task "Nordlicht recruiters", Run now | Succeeded; stages "Searched ReMa sources · 5 sources", "Answer written from the sources"; Search scope "Company, People"; Sources consulted "Wikidata, Wikipedia, Company websites, Company career sites"; shown in Settings → Recent searches. |
| 13 | External browser | The E2E browser log was not written during any search (last entry before the runs). |

## 3. Problems found in the in-app runs and fixed

| Problem | Fix |
|---|---|
| "LLM Engineer" postings were dropped for "AI jobs": the plan kept only four role variants, cutting the fifth | The role plus every variant of its family (up to five); the screenshot test now includes the Hacker News posting |
| Salary cells showed the source's sentence ("Salary: EUR 95,000 gross per year.") | Salaries ReMa parsed are shown normalized ("€95k / year"), the source's words otherwise |
| Per-posting notes ("also listed on", "whether it is still open is not stated") were collected but never shown | Shown in the Status column (the "salary not stated" note is left to the Salary column) |
| A merged listing said "Verified posting" and "ReMa could not open the page" | A checked duplicate's check replaces the failure note and brings its caveats ("checked on Hacker News"); salary caveats stay with the salary they describe |
| The research step was labelled "Searched the web" (and "Web search failed") | "Searched career sources" / "Search failed" |
| "Checked 0 postings · 4 matched" for sources read through APIs | "Checked N postings (M pages opened) · K matched, L left out"; "No postings found for the request" when nothing was found |
| A search repeated within ten minutes (the Jobs MCP's stored result) was shown as retrieved now | Shown with the time its sources were read |
| A company homepage was read three times per research | Company pages are reused for five minutes |
| "Analyze jobs" was offered under research answers (their Sources list) | The Sources list does not count as job links |
| "(Search is unavailable right now.)." and "ReMa Jobs: ReMa job sources: …" | Punctuation and the double prefix fixed |
| With every source down but two feeds still in their ten-minute cache, a new request answered "no verified matching postings … from the sources searched" without naming the source that failed | The answer names the sources that could not be searched ("Not reachable right now: Hacker News: Who is hiring? — postings listed only there may be missing."), in listings and in empty answers |

## 4. Acceptance criteria (§62)

| # | Criterion | Evidence |
|---|---|---|
| 1 | Job search works with Brave, Tavily and SearXNG unconfigured | E2E 3–7, 11; `the_screenshot_request_works_with_no_search_service…` |
| 2 | The user never selects a search provider | Settings "Automatic"; the service is under Advanced, optional (E2E 1) |
| 3 | Search follows the selected model automatically | E2E 3–6 (four engines, no setting changed); Settings follows the default model (E2E 6) |
| 4 | OpenAI native search used | E2E 5; `builds_stateless_requests_with_web_search` |
| 5 | OpenAI current-data queries force a search | `tool_choice: required` (E2E 5); a reply without a search is retried, then not used (`a_model_that_does_not_search_…`) |
| 6 | OpenAI domain restrictions | 20 allowed domains (E2E 5) |
| 7 | Anthropic web search used | E2E 4, 10 |
| 8 | Anthropic domain restrictions | 20 / 5 allowed domains (E2E 4, 10) |
| 9 | Anthropic failure falls back transparently | E2E 7; `provider_search_that_is_unavailable_falls_back…` |
| 10 | Local models can use search as a server-side ReMa tool | `rema_career_search` / `rema_read_page` (`local_models_get_remas_web_tools_for_other_questions`); job and research requests search before the model answers (E2E 3, 9) |
| 11 | Unsloth's own web search used automatically | E2E 6; `unsloth_runs_only_its_web_search_and_reports_it` |
| 12 | Other local models have a ReMa fallback | E2E 3, 9 |
| 13 | ReMa Jobs MCP is the primary job layer | Every job search runs the Jobs MCP ("ReMa Jobs" first in every engine name) |
| 14 | Scoped to jobs, ATS, employers, companies, professional research | Scopes and the career registry; domain filters from it; no general web crawling |
| 15 | Current job requests never rely on model knowledge | Listings only from sources; the model writes after, about them (`a_model_that_does_not_search_gets_no_listings_through`) |
| 16 | Every displayed job has source evidence | Every row links its posting and names its source (E2E 3–7) |
| 17 | Duplicates across sources resolved | E2E 4–6 (Greenhouse + Hacker News → one row); merge tests |
| 18 | Individual source failures do not abort | `one_failing_source_does_not_stop_the_others`; E2E 4 (the Greenhouse page did not open, the search went on) |
| 19 | Scheduled Tasks use the same infrastructure | E2E 11, 12; scheduler tests |
| 20 | Agents use the same infrastructure | Agents run in chat, on the same path (`agent_instructions_are_sent_once…` for the composition) |
| 21 | Network Connect can use it | `router::research` and `router::search_jobs` are the public entry points; used in Phase 3 |
| 22 | No external browser during background search | E2E 13 |
| 23 | No hidden authenticated browser scraping | Only documented public APIs and permitted pages through the fetcher; no browser engine in the search path |
| 24 | `Search service = None` cannot block ReMa | E2E 1–12 with None; `a_misconfigured_optional_service_never_blocks_search` |
| 25 | The screenshot error cannot occur for a missing search API | The "No search service is set up" failure path is removed; `every_route_failing_is_an_honest_operational_error` |
| 26 | Provider outages fail over | E2E 7; Unsloth and OpenAI fallback tests |
| 27 | Complete failure is an honest operational error | E2E 8 |
| 28 | Existing features keep working | Full suites pass (531 Rust, 102 Vitest) |
| 29 | All tests pass | §1 |
| 30 | Production build succeeds | `pnpm build`; the release bundle is built in CI |

## 5. Limitations

- Live provider and source verification is a release prerequisite: the
  hosts are blocked here, so every in-app run used stand-ins that follow the
  documented formats.
- The stand-in posting pages were served from 127.0.0.1, so their links are
  named after that host in the tables.
