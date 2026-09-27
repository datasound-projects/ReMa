# Production plan — Career Search (A) and Desktop Connectors (B)

Two specifications, kept outside the repository:

- **A** — `0db2db26-ReMa_Production_Grade_Always_On_Career_Search.md` (§1–§86)
- **B** — `05d000ff-ReMa_Production_Desktop_Connectors_Google_Microsoft.md` (§1–§93)

They are implemented one after the other, A first. B starts only after A
has passed its acceptance checks. Records:
[career-search/implementation.md](career-search/implementation.md),
[career-search/validation.md](career-search/validation.md),
[connectors/implementation.md](connectors/implementation.md),
[connectors/validation.md](connectors/validation.md).

## 1. Order and why

1. **A first.**
   - Every feature uses career search: Chat, Scheduled Tasks, agents, Jobs
     MCP, Network Connect, Business.
   - A extends the Phase 2 search layer, which is fresh.
   - A's capability layer (what each model runtime can do, local or cloud) is
     what B's Gmail cloud-model disclosure (B §63–§64) builds on.
   - A's rule that web evidence never reaches mail, calendar or other tools
     (A §64) must hold before B exposes more mail and calendar tools.
2. **B second.** It is packaging and provider work: build configuration,
   OAuth hardening, release checks. Nothing in A depends on it.

## 2. Model runtimes ReMa actually has (A §9, §21, §69)

| Connection in Settings | Transport | Authentication | Native search |
|---|---|---|---|
| OpenAI — API key | Responses API (`llm/openai_responses.rs`) | API key | hosted `web_search` |
| OpenAI — ChatGPT account | `codex app-server` (bundled Codex 0.157.1, private `CODEX_HOME`) | ChatGPT sign-in inside Codex; ReMa never sees a token | Codex `web_search` modes (`live`, `indexed`, `cached`, `disabled`) |
| Anthropic — API key | Messages API (`llm/anthropic.rs`) | API key | server tool `web_search_*` (+ `web_fetch_*`) |
| Anthropic — Claude Console | Messages API | short-lived Console token from Anthropic's `ant` CLI (the official OAuth client) | same server tools |
| Google Gemini — API key | Gemini API | API key | Google Search grounding |
| OpenAI-compatible server (Ollama, LM Studio, llama.cpp, vLLM, MLX servers, Unsloth Studio …) | Chat Completions | optional key | none, except Unsloth Studio's own `web_search` tool |

Not supported by ReMa, so not tested: the OpenAI Agents API, the Claude
Agent SDK / Claude Code runtime, and a Claude.ai consumer login. The
Claude Console connection signs in through the `ant` CLI and yields API
credentials for the Messages API. No consumer token is ever reused (A §27).

## 3. Gap analysis of A against the code (at 9ef3263)

| A § | Requirement | Before | Work |
|---|---|---|---|
| 5–6, 58, 67 | Authentication separate from capability; `ProviderRuntimeCapabilities`; probe after connecting | Capability was inferred from provider kind (`native::supported`); Unsloth probed | New capability layer per runtime; learned facts (organization disabled search, workspace policy) with expiry; metadata-only probe when a provider is added or changed and at start |
| 7 | First-run bootstrap of the search infrastructure | Parts were created lazily | `career_search::bootstrap` at start, logged, no wizard |
| 13 | Provider domain limits | 20 domains for every provider | Provider caps (OpenAI 100) applied where the request is built |
| 14–15 | Verify `web_search_call`; keep sources, annotations, ranges | Verified; ranges and the new `queries` field dropped | Parse `action.queries`, `incomplete`; citations keep their ranges and quoted text |
| 17–19 | ReMa's Codex sessions default to `web_search = "live"` | Runtime default `disabled`, `live` per thread for chat | Runtime default `live`; every thread states its mode (`disabled` for extraction and private-data answers); Codex's `tools.web_search` domain filter and location now used |
| 22–23 | Anthropic tool versions kept up to date; `allowed_callers` | `web_search_20260209` / `20250305` hard-coded | One table: `web_search_20260318` + `web_fetch_20260318` for models with dynamic filtering, `allowed_callers: ["direct"]` for models without programmatic tool calling, `20250305` otherwise; a 400 asking for `allowed_callers` is retried once direct |
| 25, 54 | Organization-disabled search → ReMa evidence | Required steps fell back; chat answered without any search | The chat answer continues with ReMa's career tools; the fact is remembered |
| 34, 37, 44–47 | Discovery → pages → extraction → chunks → ranking | Structured sources only; page text cut at N characters | `SearchDiscoveryProvider` (ReMa Jobs, company sources, DuckDuckGo HTML as one provider); shared content extractor (Business reuses it); section chunks ranked with BM25 |
| 41–42 | Scope CONTRACTS; `supports_contracts`, `ats`, `enabled` | Contracts folded into jobs | `Scopes.contracts`; registry fields; contract marketplaces |
| 48–49 | One citation model; unknown references never render as sources | Sources list per answer | `Citation` for every provider; answers are checked against the table |
| 50–51 | `valid_through`, `verified_at`, application address, source job id | Partly | Carried through listings |
| 31, 63–65 | Local tools return evidence; query privacy; no exfiltration | Tools could open any address | `rema_read_page` opens only pages ReMa found in this answer or the user named; queries are trimmed of contact data |
| 66 | `[career-search]` diagnostics | Route reports only | One log line per search, no secrets and no query text |
| 81 | Migration of `search_service` | Setting kept, optional | Verified and recorded, nothing deleted |

Already in place from Phase 2 and re-verified here: requirement and
scopes, the registry-based domain filter, the Jobs MCP as the primary job
layer, native search verification, harness-driven search for local models,
Unsloth's `web_search`, failure isolation, the honest outage message,
Settings "Career Search — Automatic", Scheduled Tasks and agents on the
same router, SSRF protection in the fetcher.

## 4. Checklist A

Verified: automated tests pass and the item was exercised in the running app
([validation §6](career-search/validation.md)). Numbers in the evidence
column are rows of the in-app table (validation §6.2).

| # | Item | Status | Evidence |
|---|---|---|---|
| A1 | Capability layer, probe, bootstrap, diagnostics | Verified | capability and bootstrap tests; rows 1, 2, 6, 16 |
| A2 | Codex live default, per-thread modes, domain filter | Verified | Codex tests; rows 3, 4, 5, 12 (real Codex binary; the code-mode search found in row 3 is fixed) |
| A3 | Anthropic tool table, `allowed_callers`, organization fallback | Verified | Anthropic and fallback tests; rows 5, 6, 8, 14 |
| A4 | OpenAI limits, `queries`, citation ranges | Verified | Responses tests; row 7 |
| A5 | CONTRACTS scope, registry fields, contract sources | Verified | registry and requirement tests (in-app contract searches need the blocked marketplaces) |
| A6 | Discovery providers (DuckDuckGo one of several) | Verified | discovery tests; row 10 |
| A7 | Content extractor, chunks, BM25 evidence | Verified | extractor and evidence tests; row 10 (banner and hidden text never reached the model) |
| A8 | Citation model and integrity check | Verified | citation tests; rows 3–9 (every row links its posting) |
| A9 | Job verification fields | Verified | `posting_facts_survive_the_merge_and_show_in_the_table`; rows 4, 12 |
| A10 | Local tools: evidence, provenance, query privacy | Verified | tool tests; row 10 (a page ReMa did not find was refused) |
| A11 | Tests (§69–§80 automated) | Verified | 684 Rust (3 ignored), 118 Vitest; mapping in validation §6.1 |
| A12 | In-app E2E matrix | Verified | validation §6.2–§6.4 |
| A13 | Docs, commit, push, CI | Done | this file, implementation §8, validation §6; CI runs on the push |

## 5. External blockers (need the user or a normal network)

What this container cannot show, and what closes each gap:

- **Live OpenAI and ChatGPT searches (A §70, §71).** No OpenAI API key or
  ChatGPT account is available, and `api.openai.com`, `chatgpt.com` and
  `auth.openai.com` are blocked. The Codex path ran with the real bundled
  Codex 0.157.1 binary against a stand-in built from Codex's own source,
  with a seeded test sign-in. Needs: one ChatGPT sign-in and one API key on
  a normal network, then the §70 and §71 prompts.
- **Live Claude search (A §72).** No Anthropic API key or Console login is
  available. The Messages requests were checked against a stand-in. Needs:
  a key, then the §72 prompt, once with an organization that has web search
  turned off.
- **Live sources.** Job boards, ATS APIs, Wikidata and DuckDuckGo are
  blocked here; ReMa's own sources and discovery ran against local
  stand-ins in their documented formats.
- **Clean machine (A §80).** Simulated with fresh data directories and a
  fresh Codex home in this container, not on a second computer.
