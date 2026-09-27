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

## 6. Production-grade career search (specification A)

Specification: `0db2db26-ReMa_Production_Grade_Always_On_Career_Search.md`
(§1–§86); implementation in [implementation.md §8](implementation.md), plan
and checklist in [../production-plan.md](../production-plan.md).

Environment as in §1–§5, plus: the **real bundled Codex 0.157.1 binary**
as the ChatGPT-account runtime, against the stand-in in HTTPS mode (port
8778, a throw-away test CA trusted through `SSL_CERT_FILE`), with ReMa's
private Codex home seeded by `scripts/e2e/seed-codex-home.py` (test tokens
for a test workspace — the ChatGPT sign-in itself cannot run here because
auth.openai.com and chatgpt.com are blocked); a scripted stand-in for
Anthropic's `ant` CLI (Claude Console sign-in); the DuckDuckGo HTML stand-in
(`REMA_DEV_DDG_URL`). No provider credential was used; every key typed in
was a test string answered by the stand-ins. Brave, Tavily, SearXNG and
SerpAPI were never configured.

### 6.1 Automated checks

| Check | Result |
|---|---|
| `cargo fmt --check` | clean |
| `cargo clippy --all-targets --locked -- -D warnings` | clean |
| `cargo test --locked` | 684 passed, 3 ignored (explicit-only tests); 685 after the CI fix below |
| `pnpm lint`, `pnpm typecheck` | clean |
| `pnpm test` | 118 passed (23 files) |
| `pnpm build` | built |

| Spec | Tests |
|---|---|
| §5–§6, §58, §67 capability | `capability_follows_the_runtime_not_the_provider_name`, `refusals_are_told_apart_from_passing_errors`, `diagnostics_carry_no_secrets_or_queries`, `a_provider_that_refused_its_search_is_not_asked_again_for_a_while` |
| §7, §81 bootstrap and migration | `the_bootstrap_line_names_the_parts_and_no_user_data`, `an_old_search_setting_never_decides_whether_remas_search_runs` |
| §13–§15 OpenAI | `builds_stateless_requests_with_web_search` (100-domain cap), `reports_url_citations` (ranges), `reads_the_queries_of_a_search_and_its_outcome` |
| §17–§19 Codex | `the_runtime_searches_live_by_default`, `a_searching_thread_gets_the_career_sites_and_the_place`, `reads_every_query_of_a_codex_search`, `code_mode_models_get_web_search_without_code_execution`, `codex_web_tool_results_count_as_reported_sources` |
| §22–§25 Anthropic | `picks_the_web_tool_versions_each_model_has`, `a_search_the_organization_turned_off_continues_with_remas_tools`, `a_model_told_to_search_directly_is_asked_again_directly` |
| §34–§37 discovery | `reads_duckduckgo_results_without_ads_or_its_own_pages`, `queries_carry_what_is_sought_not_the_users_words`, `registry_sites_scope_the_first_searches_and_networks_are_not_read`, `research_with_few_sources_discovers_reads_and_ranks_pages`, `discovery_follows_robots_rules_and_is_never_the_only_route` |
| §41–§42 scopes, registry | `current_career_questions_require_search` (contracts), `contract_requests_search_the_marketplaces_first`, `every_source_is_well_formed_and_enabled` |
| §46–§47 extraction, evidence | `leaves_out_banners_ads_and_hidden_text_and_keeps_tables_and_dates`, `dates_come_from_json_ld_or_a_time_element`, `chunks_follow_the_page_structure`, `ranking_brings_the_passage_that_answers_the_question` |
| §48–§51 citations, verification | `references_the_table_does_not_have_never_look_like_sources`, `links_to_pages_that_are_not_sources_lose_the_link`, `provider_citations_map_to_the_same_model`, `posting_facts_survive_the_merge_and_show_in_the_table` |
| §31, §63–§65 local tools | `pages_are_read_only_when_remas_search_found_them_or_the_user_gave_them`, `a_refused_page_is_shown_by_its_host_only`, `the_company_a_model_names_is_researched_by_name` |
| §70, §73, §76 prompts | `recognizes_job_searches` (the counts and "newly posted" are not part of the role) |
| §56–§57 Settings | Vitest `CareerSearchSection` (automatic, no key, capability lines for a local server, Anthropic and a ChatGPT account) |

### 6.2 In-app end-to-end (debug build)

Run 1 started from an empty data directory. F1 and F2 each started from an
empty data directory again (§80).

| # | Spec | Scenario | Observed |
|---|---|---|---|
| 1 | §7, §77, §80 | First start, nothing configured | `[career-search] bootstrap router=ready registry_sources=34 discovery=jobs-mcp,company-sites,duckduckgo,optional-service …`; Settings → Career Search "Automatic", "Nothing to set up. No API key required.", ReMa job sources and company research ready, no search service. |
| 2 | §58, §67 | ChatGPT sign-in (Codex, account already signed in) | Connected as ana@example.com · Plus, GPT-6-Astra; `ready provider=openai auth=chatgpt runtime=codex-app-server native_search=true native_mode=live live=true domain_filter=true … inference=ready jobs_mcp=ready` at once, without a search. |
| 3 | §71 | "Find current AI jobs in Vienna." with GPT-6-Astra | **First run failed the test**: Codex 0.157's code-mode models got no usable web tool ("code-mode host is disabled"), so ReMa reported "the model answered without searching the web … Used ReMa Jobs instead" — correct fallback, but no live Codex search. Fixed (implementation §8.3). After the fix: Codex offered `web.run`, searched with `external_web_access: true`, 20 career domains and location Vienna; `webSearch` items with results reached ReMa; "Searched career sources · ReMa Jobs + ChatGPT web search · 4 searches · 2 pages read · 6 postings" (`executed=true`). The user enabled nothing. |
| 4 | §78, §79 | "Find me most recent AI jobs in Vienna Austria with salary starting from 85k a year." (Codex) | 5 postings, "salary from 85,000/year"; "Salary: 3 state a salary from 85,000/year · 2 do not state one (shown, not confirmed to meet the minimum). Not shown: 1 below the salary minimum · 1 closed." No search-service error. |
| 5 | §75 | Same chat: Codex → Claude Sonnet 5 → local model → Codex, "Find current AI jobs in Vienna." each time, model changed in the composer only | Claude: `web_search_20260318` + `web_fetch_20260318`, 20 domains, Vienna, 6 postings; local: ReMa Jobs, 5 postings; Codex again: `web.run`, 6 postings. No Settings visit, no restart. |
| 6 | §25, §54, §72 | Claude with web search turned off for the organization (400) | "Anthropic web search: Anthropic returned an error (400): web search is not enabled for this organization. Used ReMa Jobs instead." with 5 postings; the next request did not ask Anthropic's search again: "Anthropic web search: turned off by the provider (web search is not enabled for this organization). Used ReMa Jobs instead." (`native_search=false`). |
| 7 | §70 | OpenAI API key (connection changed from ChatGPT), "Find 10 AI Engineer positions currently open in Vienna, posted recently, and cite the direct source for every role." | Responses request: `web_search`, 20 allowed domains, location Vienna, `tool_choice: required`, `include: web_search_call.action.sources`; `executed=true`; 6 postings, each row linking its posting; nothing invented to reach 10. |
| 8 | §72 | Claude Console sign-in (ant CLI stand-in) | `ready provider=anthropic auth=claude-console runtime=messages …`; same query searched with Anthropic's tools (Console token as bearer), 6 postings. |
| 9 | §73, §74 | "Find 10 currently open AI Engineer jobs in Vienna." with the plain local model (no tool calling) | ReMa searched before the model: ReMa Jobs, 7 searches, 5 postings with source links; the model's text came after the table. |
| 10 | §31–§37, §63–§65 | General question to the local model: "What is Wien AI Labs generally known for?" | The model called `rema_career_search` (company "Wien AI Labs"): Wikidata knew nothing, so DuckDuckGo ran (`site:kununu.com …`, `site:crunchbase.com …`, then the open query; robots.txt read first); 3 candidates, 2 pages read, the robots-closed page not read (`[career-search] discovery providers=DuckDuckGo candidates=3 pages_read=2 pages_failed=1`). The model then read the team page (allowed: ReMa found it) and tried `https://collector.example/upload?cv=1` (refused). The model's last request held the team table ("Sophie Lehner, Head of AI") but neither the cookie banner nor the hidden injected instruction. |
| 11 | §32 | Unsloth Studio model, same jobs query | Search step with `enable_tools: true`, `enabled_tools: ["web_search"]` only, `permission_mode: off`; answer step without tools; 6 postings. |
| 12 | §60, §76 | Scheduled task "Every hour: Find newly posted AI jobs in Vienna.", Run now, then the scheduler | **First Run now showed "1 current posting for newly posted AI"**: the role kept "newly posted". Fixed. Run now: succeeded, 6 postings, "Searched with ReMa Jobs + ChatGPT web search (4 searches)"; context: Search used · 4 searches, scope Jobs, sources ReMa Jobs, Arbeitnow, The Muse, Hacker News, ChatGPT web search. At 18:00 the scheduler fired it ("Scheduled", 14 s, 6 postings, `executed=true`); next run 19:00. |
| 13 | §81 | Upgrade: `websearch.kind = none` in the settings table before start | `[career-search] migration optional_service=none cleared=true`; the rows were gone; search unaffected. |
| 14 | §80 | F1: empty data directory, Claude API key only | Searched: "ReMa Jobs + Anthropic web search", 6 postings. |
| 15 | §80 | F2: empty data directory, local model only | Searched: ReMa Jobs, 7 searches, 5 postings. |
| 16 | §66 | Diagnostics | Every search logged one `[career-search] search …` line (runtime facts, requirement, scopes, executed, counts, fallback, duration); no query text, page text or credential. |
| 17 | — | External browser | Opened only for the Claude Console sign-in; never during a search. |
| 18 | §64 | Model requests | The stand-in flagged no request carrying a credential, the team page's contact details or the planted instructions. |

### 6.3 Problems found in the in-app runs and fixed

| Problem | Fix |
|---|---|
| Codex 0.157's current models (`code_mode_only`) could not search: their web tool lives in code mode, which ReMa keeps off | `features.code_mode={enabled=false,direct_only_tool_namespaces=["web"]}`: `web.run` offered directly, code execution still off; `web.run` results count as reported sources |
| An account that was already signed in to Codex was not provisioned after connecting | Provisioned on that path too |
| "turned off by the provider: Anthropic web search: Anthropic returned an error (400): …" repeated the engine | Only the provider's own words are kept: "turned off by the provider (web search is not enabled for this organization)" |
| Settings said "own web search (live) · live" for Codex | Said once |
| "newly posted AI jobs" searched for the role "newly posted AI"; "Find 10 AI Engineer …" for "10 AI Engineer" | Recency words and counts are not part of the role |
| A local model's `rema_career_search("Wien AI Labs AI team Vienna")` never searched the company: its name had no "at/about" before it, and unrelated employers' postings made the result look sufficient | The tool takes the company in question (`company`); keyword queries with job nouns count as job searches |
| A refused page read was not visible | Shown in the activity by its host only |
| Scheduled job runs showed a second "Searching the web" stage after the assessment | Provider searches count inside the stage that runs them |
| Unsloth's detail read "web_search (Unsloth Studio)" in logs and Settings | "web_search" (the runtime names Unsloth Studio) |
| **CI** failed `business::tests::a_total_outage_is_a_failure_not_an_empty_market` on the first Spec A push: the shared test state left DuckDuckGo discovery on its public address, so on a runner with internet access the "outage" still found employers (the container blocks DuckDuckGo, so it passed locally) | The test state points discovery at a closed local address, like ReMa's job sources. The whole suite was then run through a proxy that refuses and records every request: it also found a test fetching `http://169.254.169.254/latest` (test fetchers accept local addresses) and Google's real token endpoint. Link-local addresses (cloud metadata) are now refused in every mode, and the test state's Google and Microsoft endpoints are closed local addresses. CI green on e3138be |

### 6.4 Acceptance (§69–§82)

| § | Requirement | Evidence |
|---|---|---|
| 69 | Every supported runtime tested separately | OpenAI Responses (7), Codex/ChatGPT (3–5, 12), Anthropic API key (5, 6, 14), Claude Console (8), Unsloth (11), OpenAI-compatible local (9, 10, 15). Not supported by ReMa, so not tested: Claude Agent SDK, OpenAI Agents API |
| 70 | OpenAI: sources, real search, URLs, citations, nothing invented | 7 |
| 71 | Codex: search configured automatically, live search executes, nothing enabled by the user | 2, 3 (after the fix) |
| 72 | Claude: real web search or ReMa fallback, no setup | 5, 6, 8, 14 |
| 73 | Local: ReMa searches, URLs, pages, evidence, model receives it, sources | 9, 10 |
| 74 | Weak local model | 9 (no tool calling; search before inference) |
| 75 | Codex → Claude → local → Codex | 5 |
| 76 | Scheduled task: Run now and the scheduler | 12 |
| 77 | Zero configuration | 1–15 (no service at any point) |
| 78 | UI regression prompt | 4 |
| 79 | Salary statuses | 4 |
| 80 | Fresh machine: Codex, Claude, local | Run 1 (Codex), F1 (Claude), F2 (local) — fresh data directories in this container |
| 81 | Upgrade | 13; `an_old_search_setting_never_decides_whether_remas_search_runs` (Brave kept under Advanced) |
| 82 | No false guarantee | A source or provider that fails is named and skipped (6); all routes failing is the honest outage message (§4 row 27) |

### 6.5 Limitations

- Live provider searches still need a real key or account on a normal
  network: OpenAI, ChatGPT and DuckDuckGo hosts are blocked here, and no
  Anthropic key was available. The Codex runtime is the real binary; its
  backend, the ChatGPT workspace check and `web.run`'s search endpoint are
  stand-ins built from Codex 0.157.1's own source.
- The ChatGPT and Claude Console sign-ins were not performed against the
  real services (the Codex home was seeded with test tokens; the `ant` CLI
  is a script).
- DuckDuckGo's HTML endpoint may refuse automated clients or disallow them
  in robots.txt; ReMa then skips it (logged, never shown) and answers from
  its other routes.
