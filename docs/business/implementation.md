# ReMa Business — implementation record

ReMa Business (Appendix B of `ae663aa0-ReMa_Network_Connect_and_Business_Implementation_Guide.md`,
B0–B40) finds organizations that may buy what the user offers, finds
advertised contract work, plans go-to-market experiments and keeps one
commercial pipeline, separate from job Applications. It is the second half
of Phase 3 of [the implementation plan](../implementation-plan.md) and builds
on [Network Connect](../network-connect/implementation.md). Validation:
[validation.md](validation.md).

Nothing in Business sends a message, submits a proposal, buys anything or
contacts anyone. Drafts are local text.

## 1. Implemented workflows

**Business Profile and offers (B3–B5).** One Business Profile and any number
of offers (service, digital product, hybrid).

- An offer can be entered by hand. Name, what it does and the problem are
  enough, with no website and no social network.
- It can also be read from a product URL. The crawl is bounded (8 pages,
  depth 2, 3 redirects, 2 MB a page, 120,000 characters, 60 s), stays on the
  product's site or tenant and follows robots rules.
- A document can be read the same way.
- Every extracted claim needs an exact quote from a page that was read, and
  keeps its page, excerpt and retrieval time. Its status is observed,
  user-confirmed, hypothesis or unknown. Marketing figures are flagged "not a
  proven result" and trials "not a permanent promise".
- Reviewing a draft creates an immutable version. A later website refresh
  proposes changes and never overwrites the user's corrections.
- In Chat, "my Support Workspace" picks that offer (the longest name wins).
  An archived or unreviewed offer produces an error and is never swapped for
  another. "My product" with several offers asks which one.

**Find Clients (B6–B11).** Companies are found by place and industry or size
(Wikidata's query service), by the model's web search (only pages the search
engine reported) and from job postings, used as observed signals only. The
companies' own sites are read within robots rules.

- Hard checks (location, exclusions) pass, fail or stay unknown.
- A reproducible fit measure uses the versioned policy
  `business-fit-2026-09-27`:

  | Criterion | Weight |
  |---|---|
  | use case / problem | 0.35 |
  | technical fit | 0.25 |
  | industry | 0.20 |
  | scale | 0.20 |

  Unknown lowers coverage, never the score. Below 60 % coverage the result
  reads "Insufficient evidence" instead of a number.
- Buyer roles are shown with "authority unknown". Named people come only
  from public pages that name them.
- "Verified buying intent" reads "None observed" unless a source shows a
  purchase.
- "Permission to contact" is always "Not determined by ReMa".
- Results are grouped as confirmed, needs verification and excluded.
- Saving a prospect creates a New Lead. Prospects already saved show
  "In Pipeline".

**Find Contract Work (B12–B14).** The request becomes visible strict
criteria: engagement, skills and places (DACH expands visibly).

- The rate threshold can be "above" or "at least", with a unit and a
  currency.
- Hourly rates are compared with a daily threshold only when the user gives
  hours per day.
- Another currency is never converted without a sourced exchange rate.
- The duration must lie within the requested range.
- Listings are classified as freelance, contract, fixed-term employee,
  employment or needs verification.
- Agencies keep the end client undisclosed.
- "Remote" is not "worldwide".
- A saved contract becomes a Contract opportunity, never an employment
  Application.
- The listing check changes only the listing status. A closed listing keeps
  its notes, next step and stage.

**Pipeline (B15, B16, B23, B29).** The stages are New Lead → Qualified →
Contacted → Discussion → Proposal → Won / Lost, and they move only by the
user's action.

- Contacted and later stages rest on user-reported activity, which cannot be
  dated in the future.
- Won is "an accepted engagement, not money received".
- An opportunity's identity is company + offer + use case, so saving twice
  is idempotent.
- Stale edits are refused (revision check). Research never overwrites what
  the user wrote.
- Do not contact can apply to a company or to a person or buyer role at one
  company. Only the user can lift it, and lifting leaves a record.
- Deleting a contact deletes the drafts addressed to it. For a named person
  it also removes their name from stored research, notes and history, and
  keeps a record without the data.
- Business searches that did not finish are shown as such, next to the
  earlier results.

**Go-to-Market Studio (B17–B23).** A plan belongs to one offer version.

- Segments stay hypotheses. The categorical alternatives every buyer has
  (keeping the manual process, building it internally, an agency or
  consultant) and any researched products keep "unknown" pricing unless a
  source states it.
- Positioning uses reviewed facts only and lists untested claims.
- The plan also holds channels and target accounts (from Find Clients).
- Drafts ask instead of asserting, and never invent names, relationships or
  contact details. They are refused for anyone or any role marked Do not
  contact.
- Experiments have a fixed account cohort and variants, assigned randomly by
  account or by hand. They are frozen when they start; an amendment is a new
  version.
- Metrics come only from recorded activity and are counted per account.
  Replies before contact or outside the window are not attributed, and one
  win counts once.
- The limitations shown include small cohorts (under 20), user-reported
  data and non-random assignment.
- A contact recorded by moving a stage can be attributed to an experiment,
  with the account's assigned variant.

**Chat and scheduling (B25, B26).**

- Detection: a request that names the user's offer, or says "my/our …
  offer/product/service", or asks for buyers or contract work, runs Find
  Clients or Find Contract Work first (hiring words keep job searches
  apart).
- Output: the result table appears as ReMa made it. The model then writes a
  short answer from `<business_results>` (a data block with no tools) under
  fixed rules: fit is not buying intent, public visibility is not permission,
  nothing may be invented, and no messages are sent.
- Tool-capable models can call read-only Business tools. Changes need the
  user's approval, and no sending tool exists.
- A scheduled prompt that names an offer runs the same research, with
  run-history stages and the result as the run output. It never changes the
  pipeline.

## 2. Changed files and migrations

| Area | Files |
|---|---|
| Domain | `src-tauri/src/business/`: `model.rs`, `offers.rs`, `ingest.rs`, `text.rs`, `clients.rs`, `fit.rs`, `locations.rs`, `contracts.rs`, `pipeline.rs`, `gtm.rs`, `experiments.rs`, `render.rs`, `service.rs`, `store.rs`, `tools.rs`, `tests.rs` |
| Commands | `src-tauri/src/commands/business.rs` (overview, offers, describe, find clients/contracts, cancel, last results, pipeline, stages, activity, suppression, deletion, listing check, reassess, plans, drafts, experiments, metrics) |
| Chat, scheduler | `services/chat.rs` (`business_then_answer`, detection before job search), `services/scheduler.rs` (`business_task`) |
| Policy | `network/policy.rs`: the Business purposes `ClientAcquisition`, `ContractSearch` and `GtmResearch` next to `ProfessionalResearch` |
| Frontend | `src/pages/BusinessPage.tsx` (tabs Find Clients, Find Contract Work, Go-to-Market Studio, Pipeline; Business Profile); `src/components/business/*`; `src/hooks/useBusiness.ts`; `src/services/businessService.ts`; `src/styles/business.css`; navigation entry below Network Connect |
| Migration `0012_business.sql` | `business_profile`, `business_offers`, `business_offer_versions`, `business_research_runs`, `business_opportunities`, `business_assessments`, `business_activities`, `business_suppressions`, `business_drafts`, `gtm_plans`, `gtm_experiments`, `business_redactions` |
| Migration `0011_network_connect.sql` | LinkedIn and XING in the connector tables (Network Connect) |

## 3. Shared components reused

- Network Connect's research layer: companies from Wikidata's entity search
  and query service, company pages, entity resolution (conservative;
  subsidiaries on one domain are kept apart), evidence and the provider data
  policy.
- The career-search layer from Phase 2: the router for job postings, the
  model's own web search checked against reported pages, and the safe fetcher
  in `rema_mcp/fetch.rs`. The fetcher resolves only to public addresses,
  checks every redirect hop, bounds size and redirects, and follows robots
  rules.
- The Jobs MCP engine for contract listings and listing checks.
- Run history from Phase 1 for scheduled Business research.
- The connector registry. A LinkedIn identity is only reported and never
  used.
- Chat streaming, agents and conversation turns.

No new search engine, search-provider setting, API key, credential store or
scheduler was added.

## 4. Provider capabilities unavailable or not verified

- **LinkedIn**: a sign-in grants identity only, and the policy denies every
  Business purpose for LinkedIn data. Sales enrichment would need LinkedIn's
  Sales Navigator Application Platform (SNAP), which is not integrated.
- **XING**: no integration. Member data is denied for commercial purposes
  unless an agreement permits it.
- **Live web**: Wikidata, company sites, job boards and model web search were
  exercised against local stand-ins only (the hosts are unreachable here).
- **Local model**: the Unsloth stand-in answered without searching, and every
  run reported this ("the model answered without searching the web, so none
  of its answer could be verified").
- There is no CRM sync, e-mail sending, advertising, payment or proposal
  submission, by design.

## 5. Open prerequisites before release

- Re-check the official pages cited in B40 on a normal network: OWASP SSRF
  and LLM prompt-injection guidance, LinkedIn's API terms and SNAP, XING's
  API terms, and the regulator pages on direct marketing.
- **Privacy and legal review**:
  - B29 retention of prospect and contact data;
  - the lawful basis and information duties for public professional data
    about people;
  - national rules on unsolicited electronic marketing (ReMa only prepares
    drafts; the user sends them);
  - the XING and LinkedIn terms for any future integration.

  None of this is decided in code beyond the conservative defaults above.
- Network Connect's own prerequisites (LinkedIn app, native PKCE, approval of
  `r_1st_connections`) do not affect Business.

## 6. Known limitations and safe fallbacks

- Fit matching is rule-based: key terms and exact quotes, no semantic model.
  Wording far from the offer's terms is missed, and the result says what is
  unknown rather than guessing.
- Places come from Wikidata and company pages. A search radius is shown as
  unavailable, because there are no reliable coordinates and the device
  location is never used.
- Prompt injection is handled by structure, not by a classifier:
  - page text reaches a model only as delimited data, with the instruction
    to ignore instructions in it;
  - claims need exact quotes;
  - answers have no tools;
  - everything is reviewed before use;
  - no action tool exists.
- Metrics are only as good as what the user records. They are labeled
  user-reported, and small or non-random cohorts show their limitations.
- If website reading fails, manual offer entry stays available. If every
  source fails, the run is marked as failed, not as an empty market. A
  partial failure keeps what did succeed.
