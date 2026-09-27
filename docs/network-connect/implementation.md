# Network Connect — implementation record

Network Connect (specification `ae663aa0-ReMa_Network_Connect_and_Business_Implementation_Guide.md`,
§1–§63) researches companies, their current jobs and the relevant people
behind them, and — only where a provider permits it — the user's own
connections. It is Phase 3 of [the implementation plan](../implementation-plan.md),
built on the Phase 2 career-search layer and the Phase 1 run history.
Validation: [validation.md](validation.md).

The order of trust never inverts (the guide's "Final Product Rule"):

```text
provider capability → permission → data policy → research → normalized evidence → model
```

The model reasons over data ReMa is allowed to use; it never decides what
provider data ReMa may access.

## 1. Where things live

| Area | Files |
|---|---|
| Capability registry (§6, §51, §52) | `src-tauri/src/network/capabilities.rs` — capabilities come from the connector account's granted scopes and the provider's documented offering, never from a Connect button |
| Provider data policy (§12, §39, §40) | `network/policy.rs` — one place that answers *may ReMa fetch / display / send to a model / store / derive* this data class from this source for this purpose; `POLICY_VERSION` and `REVIEWED_AT` are stored with research that relied on them |
| Planner (§20, §48, §49) | `network/planner.rs` — reads a request into a `NetworkIntent` and runs only the stages it needs (companies, jobs, people, connections); deterministic, like the career-search planner it builds on |
| Research service (§5, §33, §56–§59) | `network/service.rs` — one entry point for the page, Chat, tools and scheduled tracking; every source optional, a failing one is reported and the rest still answer |
| Companies and jobs (§14–§16, §26) | `network/companies.rs` — current openings from ReMa's job layer (the Jobs MCP through the career-search router) grouped by employer; Wikidata (entity search and the query service) for industry, headquarters and size; the model's own web search only through pages it reported |
| People (§10, §17–§19, §24, §25) | `network/people.rs` — strongest first: the posting itself (named recruiter, contact, hiring manager, reports-to), the company's own team and leadership pages, Wikidata office holders, pages the model's search reported. Nothing invented; contact details never collected; "Hiring manager" needs a source that says so |
| Entity resolution (§35) | `network/resolve.rs` — companies merge on the same normalized name or the same own domain; people only on the same name at the same company with a compatible title or the same profile, never on a name alone; places fold conservatively ("Vienna" into "Vienna, Austria", never "Vienna, Virginia") |
| Evidence and confidence (§34, §36, §37) | `network/evidence.rs` — every claim keeps source, URL, excerpt (contact details removed) and retrieval time; confidence follows fixed rules (below) |
| Permitted relationships (§9, §22, §23, §41, §57) | `network/relationships.rs` — LinkedIn Connections API only with `r_1st_connections` granted; first-degree only; matched to companies by ReMa; session memory only |
| Rendering (§29, §31) | `network/render.rs` — the unified Markdown table and connection line (Chat, run history) and the model's delimited data block, both built from the policy-filtered view |
| Chat tools (§32, §47) | `network/tools.rs` — `network_search`, `network_find_companies`, `network_find_relevant_contacts`, `network_check_connections` for tool-capable models; structured JSON of the model's view, never member data or tokens |
| LinkedIn connector (§7, §8) | `src-tauri/src/connectors/linkedin.rs` — "Sign In with LinkedIn using OpenID Connect" as a native PKCE client (no client secret), `openid profile email`, extra scopes only when the build records LinkedIn's approval and the token grants them |
| XING connector (§11) | `connectors/xing.rs` — shown as unavailable: no desktop sign-in and no new API applications; never asks for a password or cookies |
| Shared connectors | `connectors/{mod,oauth,tokens,sync,tests}.rs` — LinkedIn and XING join the one connector registry and the one OS-credential token store (migration `0011_network_connect.sql` widens the provider and connector checks; professional networks never sync in the background) |
| Commands | `src-tauri/src/commands/network.rs` — `network_capabilities`, `network_research`, `network_cancel`, `network_last_result` |
| Page | `src/pages/NetworkConnectPage.tsx`, `src/hooks/useNetwork.ts`, `src/services/networkService.ts`, `src/styles/network.css` |
| Settings | `src/components/settings/ConnectorsSection.tsx` — "Professional networks": LinkedIn (granted permissions, Reconnect, Disconnect, link to LinkedIn's app management) and XING (Unavailable, with the reason) |
| Chat and scheduling | `services/chat.rs` (`network_then_answer`, before a plain job search), `services/scheduler.rs` (company research and tracking tasks with run-history stages) |

## 2. Capabilities and data policy

LinkedIn's self-service sign-in grants identity only. ReMa therefore shows:

| State | Identity | Connection list (first-degree) | Member / company search | Second degree |
|---|---|---|---|---|
| Not connected | — | Not available | Not offered by LinkedIn to ReMa | Not offered |
| Connected, identity only | Yes | Not available: "your sign-in does not include it" | Not offered | Not offered |
| App approved for `r_1st_connections`, older sign-in | Yes | Available after signing in again ("Grant connection access") | Not offered | Not offered |
| Granted `r_1st_connections` | Yes | Available, this session only | Not offered | Not offered |
| XING | Not available (no desktop sign-in, no new API apps) | Not available | Not available | Not available |

Policy rules (`network/policy.rs`, reviewed 2026-09-27):

- **LinkedIn API data**: the member's own identity may be kept as the
  connector account. First-degree connection data (names, headlines,
  profile links) is shown on the Network Connect page for the session only
  (30 minutes, dropped at once on disconnect), matched by ReMa, never sent
  to a model (cloud or local), never stored, never exported, and only for
  professional research. A chat answer, its model and run history get only
  the number of matches ("ReMa found 2 LinkedIn first-degree connections at
  these companies …"), which names no member. A LinkedIn login is not a
  sales integration: every Business purpose is denied.
- **XING**: no integration exists; member data would be denied for
  storage, social-graph use and marketing.
- **Public sources** (company sites, public postings, Wikidata, pages a
  search engine reported): company and job facts are research output and
  may be kept; public professional information about people (name, current
  title, public profile link) may be used for professional research, never
  private contact details.
- **User-entered** data is the user's own.

A relationship question without permission is answered honestly, never
with "no connections", and the Connections API is not called. For an
identity-only sign-in: *"LinkedIn is connected for identity, but ReMa does
not currently have permission to read your connection list, so ReMa cannot
tell whom you know."*; with nothing connected: *"No professional network
that shares its connection list with ReMa is connected, so ReMa cannot tell
whom you know. It can still research relevant people."*

## 3. Research

1. **Plan**: the planner reads the company ("at X", "Is X hiring …",
   "Does X have …"), place, role, hiring intent, people functions and a
   relationship question from the request; only the stages it needs run.
2. **Companies and jobs**: the career-search router's job search (ReMa
   Jobs plus the model's own search, Phase 2) supplies current openings;
   they are grouped by employer. A request naming one company keeps only
   that company's openings and says how many others were left out.
   Industry discovery uses Wikidata's query service by place and industry;
   facts carry their Wikidata evidence.
3. **People**: from the postings, the companies' own pages (read once per
   research within robots rules), Wikidata office holders and pages the
   model's search reported. Each person gets a relevance type (recruiter,
   job poster, stated manager, department leader, team lead, other
   relevant contact) with the reason in words, and a confidence:
   - High: a posting names the person; the company's own page lists them
     in the department's leading role.
   - Medium: a public page shows the role at the company, but no source ties
     them to the vacancy (also Wikidata office holders).
   - Low: weak matches only (a title outside the function, or only the
     model's word).
4. **Connections** (only when permitted): first-degree connections from
   the Connections API, matched to the researched companies by headline;
   people who are also connections carry a relationship badge.
5. **Result**: one structured result (companies, jobs, people,
   relationships, evidence, notes, source failures), rendered as the
   unified table on the page and in Chat; the model receives only the
   policy view inside a delimited data block and is told to ignore any
   instructions in it.

## 4. Where it is used

- **Network Connect page**: capability cards per provider (with "Grant
  connection access" when LinkedIn approved more than the current sign-in
  granted), a request box with examples, result modes (companies, jobs,
  people), the unified table, a drill-down per company and person with
  evidence, retrieval times and direct links (opened in the system browser),
  and **Track this company…**, which pre-fills a daily scheduled task
  ("Track X: find its current open roles and the most relevant hiring-side
  contacts at X").
- **Chat**: company, people and relationship requests run the research
  first (before a plain job search), then the model writes a short answer
  from the policy view; the table is shown as ReMa produced it. Tool-capable
  models can call the four `network_*` tools instead.
- **Scheduled tasks**: the same service, with run-history stages ("Research
  companies, jobs and people", "Write the summary") and the result as the
  run output (Phase 1).
- **Business** uses the same public research and the same policy; LinkedIn
  and XING data never enter it.

## 5. Official documentation checked (§62 step 3, §63)

learn.microsoft.com and dev.xing.com are not reachable from this build
environment (the egress proxy refuses them), so the pages were verified
through search results that quote the official pages, and the design was
kept to what they state. Re-check on a normal network before release
(release prerequisite in the plan).

- **Sign In with LinkedIn using OpenID Connect**: scopes `openid` (ID
  token), `profile`, `email`; the older "Sign In with LinkedIn" was
  deprecated on 1 August 2023. Implemented exactly (`linkedin.rs`).
- **LinkedIn OAuth for native apps (PKCE)**: no client secret; the app
  listens on a random loopback port and LinkedIn redirects only to loopback
  addresses; LinkedIn must enable PKCE for the app through the developer's
  LinkedIn contact. Implemented; live sign-in needs that enablement.
- **Getting access to LinkedIn APIs**: only open / self-serve permissions
  without review; sales integrations need the Sales Navigator Application
  Platform (SNAP) partnership. Hence "a LinkedIn sign-in is not sales
  enrichment" in the policy.
- **Connections API**: `GET /v2/connections?q=viewer` returns first-degree
  connections of the member who granted access; requires
  `r_1st_connections`; restricted to approved developers; no
  connections-of-connections. Decoration (`elements*(to~(...))`) returns
  localized names and headline. Implemented behind the capability check.
- **XING**: "Login with XING" is a website plugin whose consumer key is
  bound to the registered domain; the official API clients state that "it is
  no longer possible to register new applications". Hence no XING sign-in
  in a desktop app.

## 6. Release prerequisites

- A LinkedIn developer app with the "Sign In with LinkedIn using OpenID
  Connect" product and native PKCE enabled by LinkedIn; its client ID in
  `src-tauri/connectors.toml` (`[linkedin] client_id`, or
  `LINKEDIN_CLIENT_ID`; see
  [connectors/registration.md](../connectors/registration.md)). Without it
  the LinkedIn card says LinkedIn is not part of that version.
- Connection-list access needs LinkedIn's approval of `r_1st_connections`
  for that app, recorded in `[linkedin] approved_scopes`
  (`LINKEDIN_APPROVED_SCOPES`); until then the capability stays "Not
  available".
- XING: nothing can be registered today; the connector stays unavailable.

## 7. Limitations

- Provider behaviour was exercised against local stand-ins that follow the
  documented request and response shapes (`scripts/e2e/mock-providers.mjs`);
  no live LinkedIn or XING account was used.
- Wikidata, company sites and job boards were local stand-ins too (the
  hosts are unreachable here).
- People search uses public, permitted sources only; people who appear on
  no such page are not found.
- First-degree connections are matched to companies by headline text, as
  the Connections API returns no employer field.
