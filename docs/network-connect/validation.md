# Network Connect — validation report

What was run and what was observed. Environment: Linux container, Xvfb
display, D-Bus session with GNOME Keyring, debug build. LinkedIn, Wikidata,
company websites, job boards and the model providers are not reachable from
this environment, so the in-app runs used the local stand-ins in
`scripts/e2e/mock-providers.mjs`: LinkedIn's OpenID Connect endpoints
(`/linkedin/oauth/v2/authorization`, `/accessToken` — which rejects a client
secret and requires the PKCE verifier —, `/v2/userinfo`) and its
Connections API (403 unless the stand-in plays an app approved for
`r_1st_connections`), Wikidata and its query service, company sites with
team pages (one of them carrying a prompt-injection line), job boards, and
the model endpoints. The stand-in logs every model request and flags
LinkedIn member names, contact details and credentials if they appear in
one. Debug-only overrides: `REMA_LINKEDIN_BASE_URL`,
`REMA_DEV_LINKEDIN_CLIENT_ID`, `REMA_DEV_LINKEDIN_APPROVED_SCOPES`.

## 1. Automated checks

Results of the final run are in the [plan's Phase 3 report](../implementation-plan.md#phase-3--network-connect-and-business-gate-passed).

The tests §60 asks for:

| §60 | Tests |
|---|---|
| Capability gating — LinkedIn identity only | `linkedin_identity_only_is_not_search_or_network_access`, `identity_only_linkedin_never_claims_no_connections` |
| — first-degree permission available | `linkedin_first_degree_only_with_the_granted_permission`, `permitted_first_degree_connections_are_matched_for_the_session_only` |
| — first-degree permission unavailable | `without_the_permission_nothing_is_fetched_and_the_reason_is_exact`, `an_approved_app_with_an_older_sign_in_asks_to_sign_in_again`, `a_relationship_question_without_access_says_so_and_checks_nothing` |
| — unsupported second-degree request | `second_degree_connections_are_never_simulated`, `people_and_relationship_requests` (planner) |
| — XING limited capability | `disconnected_and_xing_have_no_capabilities`, `xing_is_shown_as_not_available_and_opens_no_sign_in`, `xing_member_data_is_denied_for_every_purpose` |
| — provider disconnected | `disconnected_and_xing_have_no_capabilities`, `permitted_first_degree_connections_are_matched_for_the_session_only` (disconnect drops the session copy and says so) |
| Authentication — successful connect | `linkedin_signs_in_with_openid_connect_and_grants_identity_only`, `linkedin_scopes_approved_for_the_app_are_used_only_when_granted`, `asks_for_identity_only_unless_linkedin_approved_more`, `reads_the_userinfo_profile` |
| — cancel, invalid state | `linkedin_cancel_and_forged_state_connect_nothing` |
| — expired token | `an_expired_linkedin_sign_in_asks_to_reconnect_and_never_opens_one` (also: disconnect removes the account and tokens) |
| — disconnect | `an_expired_linkedin_sign_in_asks_to_reconnect_and_never_opens_one`, `permitted_first_degree_connections_are_matched_for_the_session_only` |
| — secure token handling | `tokens_live_only_in_the_credential_store`, `connections_never_reach_a_model_or_storage` |
| Company discovery — normalization, duplicates, ambiguous names | `companies_merge_by_name_or_domain_and_same_names_stay_apart`, `sparql_rows_become_companies_with_evidence`, `industry_companies_come_from_wikidata_with_their_facts` |
| — location matching | `a_city_and_the_same_city_with_its_country_are_one_location`, `criteria_exclude_confirmed_mismatches_and_state_unknowns` |
| Job integration — Jobs MCP result reused, job-to-company mapping | `jobs_group_into_their_employers`, `a_network_request_becomes_a_job_search`, `companies_then_jobs_then_people_in_one_request`, `a_named_company_keeps_only_its_own_openings`, `a_question_about_one_company_stays_with_that_company`, `tracking_a_company_lists_its_openings_without_a_role` |
| — duplicate jobs, expired jobs | the shared career-search layer: `the_employers_posting_wins_and_duplicates_merge`, `drops_closed_expired_and_unrelated_postings` (Phase 2) |
| People discovery — named recruiter, department leader | `postings_name_their_contacts_and_managers`, `team_pages_list_names_with_titles`, `functions_and_title_kinds` |
| — same name | `people_are_never_merged_on_a_name_alone` |
| — stale title, missing profile URL | `model_people_need_reported_pages_and_known_companies` (only reported, current pages count; people are kept without a profile link rather than given a guessed one) |
| — confidence calculation | `confidence_follows_the_rules`, `relevance_and_confidence_follow_the_evidence` |
| Relationship lookup — verified first degree | `reads_decorated_connections_and_matches_companies_by_headline`, `permitted_first_degree_connections_are_matched_for_the_session_only` |
| — no match, capability unavailable | `permitted_first_degree_connections_are_matched_for_the_session_only` (two checked, one matched), `without_the_permission_nothing_is_fetched_and_the_reason_is_exact` |
| — no false "no connections" | `identity_only_linkedin_never_claims_no_connections`, `without_connection_access_the_answer_never_says_no_connections` (Chat), `renders_the_unified_table_and_an_honest_connection_line` |
| Inference — explicit hiring manager, likely contact, no unsupported hiring-manager claim | `postings_name_their_contacts_and_managers`, `relevance_and_confidence_follow_the_evidence` (a public-profile engineering manager is a team lead at Medium, never a hiring manager) |
| Security — token never sent to a model | `connections_never_reach_a_model_or_storage`, `companies_jobs_and_people_are_researched_before_the_answer` (no credential in any request) |
| — page prompt injection ignored | `companies_jobs_and_people_are_researched_before_the_answer` (the answer request has no tools and does not contain the team page's planted instruction), `companies_then_jobs_then_people_in_one_request` (neither the instruction nor contact details enter the result), `team_pages_list_names_with_titles`, `the_model_gets_a_delimited_data_block`, `excerpts_are_data_without_contact_details` |
| — no authenticated scraping path | `no_authenticated_scraping_path_exists` |
| — retention policy enforced | `linkedin_connections_are_shown_for_the_session_and_never_stored_or_sent`, `keep_filters_and_counts`, `public_professional_information_is_research_output` |
| Chat — company → jobs → people | `companies_jobs_and_people_are_researched_before_the_answer` |
| — job → people | `a_job_leads_to_its_hiring_side` |
| — company → people → permitted connection check | `permitted_first_degree_connections_are_matched_for_the_session_only` (service) and the in-app Chat run (2.9) |
| — ProfileContext only when appropriate | `the_profile_is_used_only_when_the_chat_allows_it` |
| Chat tools | `structured_arguments_become_plannable_requests` |
| Page | Vitest `NetworkConnectPage`: capability states per provider, "Grant connection access" when the app may read connections but the sign-in did not grant it, request → unified table and drill-down |

## 2. In-app end-to-end (debug build)

| # | Scenario | Observed |
|---|---|---|
| 2.1 | Network Connect, nothing connected | LinkedIn "Not connected" with "Not available to ReMa now: Connection-list access — Connect LinkedIn and grant it to use it."; XING "Not available" with its reason ("XING offers no sign-in for desktop apps and no API access for ReMa. Company, job and public people research work without it."); request examples shown. |
| 2.2 | "Do I know anyone at Nordlicht AI?" (nothing connected; repeated after 2.12) | Only the stages the question needs run: "What ReMa searched" lists Companies (1 kept of 1 found · Wikidata) and Connections ("Connection list not available"). The connection line: "No professional network that shares its connection list with ReMa is connected, so ReMa cannot tell whom you know. It can still research relevant people." — never "no connections". No LinkedIn request in the stand-in log. |
| 2.3 | Connect LinkedIn | The system browser opened the authorization URL with `code_challenge` (S256), a random `state` and `openid profile email` only; the token request carried the PKCE verifier and no client secret. The card: "Connected for identity", connection list "Not available". |
| 2.4 | Same question, identity-only sign-in | "LinkedIn is connected for identity, but ReMa does not currently have permission to read your connection list, so ReMa cannot tell whom you know." No Connections API request. |
| 2.5 | "Is Nordlicht AI hiring machine learning engineers, and who should I talk to there?" | After the fixes in §3: 1 company, 1 open role (Machine Learning Engineer, from the company's Greenhouse board), 3 relevant people: Max Muster — CTO, High ("Likely relevant department leader: the Machine Learning Engineer opening belongs to their area; listed as CTO at Nordlicht AI", from the company's team page); Anna Beispiel — Head of Talent Acquisition, Medium ("Relevant recruiter: … recruiting at the company, not tied to the Machine Learning Engineer opening by a source"); Erika Muster — CEO, Medium (company leadership, a Wikidata office holder, "not tied to the … opening by a source"). "2 openings at other companies left out: the request is about Nordlicht AI". Anna Beispiel's e-mail address and phone number on the team page appear nowhere; the page's planted instruction appears in no result and in no saved record (0 rows in SQLite). |
| 2.6 | Drill-down | Company facts with Wikidata evidence and retrieval times; each person with the page that lists them, excerpt (no contact details) and time; links only where a source gave one. |
| 2.7 | Open a profile/company link | Opened through the system browser (the E2E browser log recorded the URL); nothing opened inside ReMa. |
| 2.8 | **Track this company…** | Pre-filled a daily task "Track Nordlicht AI: find its current open roles and the most relevant hiring-side contacts at Nordlicht AI."; after scheduling, **Run now** produced a run with stages and the result "1 company · 1 open role · 3 people" in run history. |
| 2.9 | App approved for `r_1st_connections` (stand-in plays the approval; `REMA_DEV_LINKEDIN_APPROVED_SCOPES`) with the older identity-only sign-in | The card explains that the app may read the connection list but the sign-in did not grant it, with **Grant connection access**; signing in again granted `r_1st_connections`. The card then lists "Available to ReMa: Identity, Your profile, Connection-list access" and "Not available to ReMa now: Second-degree connections, People search, Company search, Job search, Other members' profile details, Keeping member data". |
| 2.10 | "Do I know anyone at Nordlicht AI?" with the permission | Under "Your connections — From LinkedIn, shown for this session only; not saved in chats or run history.": Max Muster (CTO at Nordlicht AI) and Jane Example (Senior ML Engineer at Nordlicht AI), each with "Open profile"; Lena Andere at "Nordlichter Bank" is not matched. One Connections API request. |
| 2.10b | "Is Nordlicht AI hiring machine learning engineers, who should I talk to there, and do I know anyone at Nordlicht AI?" on the page | The people table of 2.5 plus the connections: Max Muster, found on the company's team page, carries the "LinkedIn first-degree connection" badge in the table; Jane Example, on no public page, appears only under "Your connections". |
| 2.11 | The same question in Chat | The answer shows the scoped table and reports only the number of connections: "ReMa found 2 LinkedIn first-degree connections at these companies. LinkedIn connection details are shown only in Network Connect during this session; they are not saved in chats or run history." That count is also all the model is given about connections. The stand-in's audit of every model request: no LinkedIn member names, no team-page contact details, no credentials. The question was repeated with the audit extended to the team page's planted instruction: `injected: false` for all three model requests (two search steps and the answer), although the team page was fetched. SQLite: no connection names (Max Muster appears only in rows from the public team page, as CTO) and no LinkedIn token. |
| 2.12 | Disconnect LinkedIn (Settings → Connectors → LinkedIn → Disconnect) | The confirmation says what is removed and that ReMa leaves the LinkedIn account entirely only through LinkedIn's permitted services. After confirming: the card is back to "Not connected"; the page's last result replaces the connection names with "LinkedIn was disconnected: the connection details it returned were removed."; the LinkedIn account row is gone from SQLite; no request went to LinkedIn. |
| 2.13 | Business Find Clients with the identity-only sign-in | The result notes say LinkedIn is connected for identity but its connection list and member data are not used to find clients; no LinkedIn API request during the search. |

## 3. Problems found in the in-app runs and fixed

| Problem | Fix |
|---|---|
| "Is Nordlicht AI hiring …" did not target Nordlicht AI (only "at X" was read as a company) | The planner also reads a company as the subject ("Is/Does/Has X hiring/have …"); tests for positive and negative cases |
| A request about one company listed other employers' openings (from the model's search) | Openings and derived companies are kept to the named company, with a note saying how many were left out (`only_at`, unit and integration tests) |
| Internal ids appeared in notes; "Vienna, Austria" and "Vienna" shown as two locations; "1 relevant openings"; a stray dash and a duplicate heading in the drill-down | Notes use names; places fold conservatively (`add_location`); pluralization; drill-down layout |
| "Track Nordlicht AI" found 0 roles (no role named) | A tracking request searches the company's openings without a role (`tracking_a_company_lists_its_openings_without_a_role`) |
| Relevance reasons read "this request belongs to their area" | Phrasing per relevance type when no specific opening is involved |
| An app approved for `r_1st_connections` with an older sign-in showed "Not available" with no way forward | `grant_available` capability state, explanation and **Grant connection access** (test `an_approved_app_with_an_older_sign_in_asks_to_sign_in_again`, Vitest) |
| After disconnecting, the page kept an empty "0 connections checked" line | The session copy is dropped and the outcome says the details were removed (and when they expire after 30 minutes) |
| The capability card's list was headed "Not available to this ReMa integration", and what it would take to get connection-list access was only in a tooltip | Heading "Not available to ReMa now"; the connection-list line states its reason ("— Connect LinkedIn and grant it to use it.", "— LinkedIn has not granted ReMa access to connection lists.") (Vitest) |
| A Wikidata office holder's reason repeated its label ("Company leadership: Company leadership (CEO); …") | The reason says where they are listed ("Listed as CEO at Nordlicht AI; not tied to the Machine Learning Engineer opening by a source"), pinned in `relevance_and_confidence_follow_the_evidence` |

## 4. Acceptance criteria (§61)

| # | Criterion | Evidence |
|---|---|---|
| 1 | Network Connect is its own navigation feature | `src/app/pages.ts`; sidebar entry (2.1) |
| 2 | LinkedIn and XING through the shared connector architecture | One registry and token store (migration 0011, `connectors/`); Settings cards (2.12) |
| 3 | Official authentication | LinkedIn OpenID Connect with native PKCE (2.3); XING not offered because no supported desktop mechanism exists |
| 4 | Never asks for passwords or cookies | No such field anywhere; `no_authenticated_scraping_path_exists` |
| 5 | Tokens stored securely, never exposed to the model | OS credential store (`tokens_live_only_in_the_credential_store`); `connections_never_reach_a_model_or_storage`; stand-in audit (2.11) |
| 6 | Capabilities represented dynamically | `capabilities.rs` from granted scopes; states 2.1, 2.3, 2.9 |
| 7 | OIDC identity is not search access | `linkedin_identity_only_is_not_search_or_network_access`; 2.4 |
| 8 | First-degree only with the approved permission | `linkedin_first_degree_only_with_the_granted_permission`; 2.9–2.10 |
| 9 | No second-degree traversal | `second_degree_connections_are_never_simulated`; no such API call exists |
| 10 | XING reflects the actual integration | Unavailable with reason (2.1); `xing_is_shown_as_not_available_and_opens_no_sign_in` |
| 11 | Retention restrictions enforced centrally | `policy.rs`; session-only connections, dropped on disconnect (2.12) |
| 12 | Discovers companies | `industry_companies_come_from_wikidata_with_their_facts`, `companies_then_jobs_then_people_in_one_request`; 2.5 |
| 13 | Reuses the job-search/MCP layer | `a_network_request_becomes_a_job_search`, `jobs_group_into_their_employers` |
| 14 | Relevant people from permitted/public sources | `people.rs` tests; 2.5 |
| 15 | Direct public profile links when confidently resolved | Links only from sources (2.6); never guessed (`model_people_need_reported_pages_and_known_companies`) |
| 16 | Companies → Jobs → People in one request | `companies_then_jobs_then_people_in_one_request`; 2.5 |
| 17 | Authorized relationship data incorporated | 2.10, 2.10b (relationship badge); `permitted_first_degree_connections_are_matched_for_the_session_only` |
| 18 | Missing permission is not "no connections" | 2.2, 2.4; `identity_only_linkedin_never_claims_no_connections` |
| 19 | People results explain why | Relevance reasons (2.5); `relevance_and_confidence_follow_the_evidence` |
| 20 | Inferred contacts are not called hiring managers | `relevance_and_confidence_follow_the_evidence`, `postings_name_their_contacts_and_managers` |
| 21 | Evidence and freshness kept | Evidence with retrieval time on every claim (2.6); `sparql_rows_become_companies_with_evidence` |
| 22 | Conservative duplicate resolution | `companies_merge_by_name_or_domain_and_same_names_stay_apart`, `people_are_never_merged_on_a_name_alone`, `a_city_and_the_same_city_with_its_country_are_one_location` |
| 23 | Callable from Chat through structured tools | `network_*` tools (`structured_arguments_become_plannable_requests`); Chat research (2.11) |
| 24 | ProfileContext reused | `the_profile_is_used_only_when_the_chat_allows_it` (the shared Profile context, only when the chat allows it) |
| 25 | Jobs MCP remains the job layer | Openings come only from the career-search router / Jobs MCP |
| 26 | No hidden authenticated scraping | `no_authenticated_scraping_path_exists`; only the documented API root is called |
| 27 | Links open correctly | System browser (2.7) |
| 28 | Page content cannot inject instructions | Team pages yield only name–title lines, and the model gets the results in a delimited block with the instruction to ignore instructions in it and no tools: `companies_jobs_and_people_are_researched_before_the_answer`, `companies_then_jobs_then_people_in_one_request`, `the_model_gets_a_delimited_data_block`; in the app the planted line reached no model request (`injected: false`, 2.11), result or saved record (2.5) |
| 29 | Existing features build and work | Full Rust and Vitest suites and the production build pass (plan's Phase 3 report); earlier phases' in-app runs repeated in the final verification |
| 30 | Automated tests pass | Same |

## 5. Not verified here

- A live LinkedIn sign-in (needs a LinkedIn app with native PKCE enabled and
  its client ID) and a live Connections API call (needs LinkedIn's approval
  of `r_1st_connections`).
- Live Wikidata, company websites and job boards (unreachable from this
  environment).
- XING: no integration is possible for a desktop app today.
