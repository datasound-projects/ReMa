# ReMa

**Career UI Harness**: a fast, lightweight desktop app built on a deterministic Rust core with an embedded LLM layer.

Rust owns application state, persistence, scheduling, validation, provider configuration and execution. LLMs are used only to reason about prompts and write the responses.

## What ReMa does

- **Chat**: a general-purpose AI chat with streaming responses, Stop, Retry, Copy, Markdown (code, lists, tables, links) and persistent conversation history ("Recents").
- **Models**: OpenAI, Anthropic, Google Gemini, and any OpenAI-compatible endpoint (Ollama, LM Studio, vLLM, …). Connect OpenAI with your **ChatGPT account** and Anthropic with your **Claude Console account** in the browser, or use API keys. Pick the model in the chat composer.
- **Scheduled Tasks**: schedule the prompt you are writing, straight from the composer. Supports one-time, daily, every N days, selected weekdays, and intervals of 15 minutes or more. A task can end on a date or after N runs. You can edit, pause, resume, delete or run tasks now, and each task keeps its run history with results.
- **Settings**: connect providers, choose which models appear in the chat, and set the default model. Connect Google Workspace (Gmail and Calendar) with one sign-in.
- **Job applications (Gmail)**: a scheduled task type that finds job-application emails, keeps one record per application (Confirmed, Application in Process, Needs Your Action, Upcoming Interview, Rejected), and shows an overview table in the task history. It can add confirmed interviews to Google Calendar and report calendar conflicts.
- **Profile**: your career context, in three independent parts. **Documents & Credentials** (the default) keeps your CV files and optional credentials (degrees, certificates, courses, licenses, badges) with an in-app preview. **Custom Profile** is an optional structured form. **Portfolio Studio** builds new CVs from 12 templates and exports them as PDF. Uploading a CV never fills the Custom Profile or opens the builder.
- **Agents**: reusable instructions for the model (a page directly below Chat). Six built-in agents (Job Search, Job Match Analyst, CV Tailoring, Interview Prep, Application Strategist, Career Research) and your own custom agents.
- **MCP (Model Context Protocol)**: connect local or remote MCP servers in Settings, turn them on, and pick them per chat. The model can then call their tools; tools that may change something wait for your approval.
- **Built-in browser**: links in Chat and task results open in a panel next to ReMa. You can collapse, maximize, dock left or right, and resize it. "Open in external browser" is always available.
- **ReMa Auto Fill**: fills the standard fields of an application form in the built-in browser from your Profile. You review and submit yourself. ReMa never submits.
- **Chat + menu**: the **+** button in the composer selects agents and MCP servers for the chat (shown as removable chips), independently of the **Profile** switch.
- **Profile context**: the **Profile** switch in the chat composer (and an option on scheduled prompt tasks) adds your CVs, credentials, Custom Profile and Portfolio Studio CVs to the request. It is off unless you turn it on.
- **Analytics**: a deterministic dashboard over every job search ReMa has run, from Chat or scheduled tasks. It opens from the **Analytics** button in the header as a panel beside the current page. You can filter and rank the jobs, compare their requirements with your Profile (skill gap), see which requirements are common or rare, and research learning resources for the gaps that matter.
- **Light and dark**: the small sun/moon button at the top-right corner switches between the light and the dark theme. ReMa remembers your choice.

## Architecture

```text
React components            src/pages, src/components
      ↓
Hooks / frontend services   src/hooks, src/services
      ↓
Generated typed IPC         src/generated/bindings.ts (from Rust, via tauri-specta)
      ↓
Thin Tauri commands         src-tauri/src/commands
      ↓
Rust services               src-tauri/src/services   (chat, chat_tools, providers, tasks, scheduler, schedule,
                                                      profile, documents, profile_import, profile_context,
                                                      portfolio, agents, mcp)
                            src-tauri/src/mcp        (MCP client on the official Rust SDK: stdio, Streamable HTTP, OAuth)
                            src-tauri/src/jobs       (job-application workflow, run by the scheduler)
                            src-tauri/src/browser    (built-in browser webview, navigation policy, Auto Fill)
                            src-tauri/src/analytics  (job ingestion, dedupe, filters, ranking, skill gap, learning)
      ↓
Persistence / providers     src-tauri/src/db (SQLite) · src-tauri/src/llm (adapters) · src-tauri/src/secrets (OS keychain)
Provider accounts           src-tauri/src/accounts (official local runtimes: Codex app-server, Anthropic CLI)
Integrations                src-tauri/src/integrations/google (OAuth, Gmail tool, Calendar tool)
```

Rules:

- **Deterministic core.** The scheduler decides when tasks run, using pure, unit-tested schedule math. The LLM never controls timing, state, credentials or configuration.
- **Provider-neutral.** Chat and scheduler use one `LanguageModel` interface. Provider, transport, authentication and model are separate concepts: the HTTPS adapters (`llm/openai.rs`, `llm/anthropic.rs`, `llm/gemini.rs`) and the Codex runtime (`accounts/codex.rs`) are transports; a connection method says how a request is authenticated; models are discovered per connection. The OpenAI adapter also serves every OpenAI-compatible endpoint.
- **Rust is the source of truth.** IPC types, commands and events are generated from Rust. The frontend never hand-writes copies of them.
- **Streaming via events.** Replies stream as typed `ChatEvent`s. Generation runs in the background, so switching chats does not interrupt it.

## Data and credentials

- **Database**: SQLite at `<app data dir>/rema.db`. On macOS that is `~/Library/Application Support/cloud.datasound.rema/`. Schema changes are versioned migrations in `src-tauri/src/db/migrations/`.
- **Profile documents**: copied into `<app data dir>/profile-documents/` under generated names. The original file name, size, SHA-256 and extracted text are stored in SQLite. The original file is never changed or moved. The interface reads a stored file only through the `rema-doc` protocol (by id, ReMa's own window only), never by path.
- **Portfolio Studio CVs, custom agents and MCP server settings** are stored in SQLite. MCP secrets (tokens, header values, environment variable values, OAuth credentials) are in the OS credential store, never in the database or the interface.
- **Credentials**: API keys and Google OAuth tokens are stored in the operating system's credential store: macOS Keychain, Windows Credential Manager, or Secret Service on Linux. They never go in the database, config files or the frontend. The UI can only save, replace or remove a key.
- **Authentication**: OpenAI — ChatGPT account (through OpenAI's Codex runtime) or API key. Anthropic — Claude Console account (through Anthropic's CLI) or API key. Gemini — API key. OpenAI-compatible endpoints take an optional key. Account credentials are kept by the provider's own runtime, never by ReMa. See [Provider accounts](#provider-accounts).

## Provider accounts

OpenAI and Anthropic can be connected with an account in the browser instead of a pasted API key. ReMa never implements a provider's OAuth protocol, never reads browser cookies or sessions, and never stores, logs or shows account tokens. There is no ReMa server: requests go from your computer to the provider.

```text
React → Tauri command → services::accounts → accounts::{codex, claude_console} → official runtime → provider
```

**OpenAI: ChatGPT account.** ReMa runs OpenAI's Codex runtime, `codex app-server`. This is the JSON-RPC interface that OpenAI's own Codex SDKs (`openai-codex` for Python, whose `login_chatgpt()` and `login_chatgpt_device_code()` call it; `@openai/codex-sdk`) and the Codex IDE extension are built on.

- **Sign-in.** **Continue with ChatGPT** calls `account/login/start`. ReMa opens the returned `auth.openai.com` page in your browser, and Codex receives the result on its own local callback.
- **Device code.** **Use a code instead** switches to device-code sign-in: ReMa shows a one-time code to enter on OpenAI's page.
- **Credentials and requests.** Codex stores and refreshes the credentials, in the OS keychain when available (`cli_auth_credentials_store = "auto"`). It also sends every model request.
- **Chat.** Each chat turn runs on an ephemeral thread. ReMa's system prompt replaces Codex's instructions, earlier messages are added as history, and every Codex tool is turned off (shell, file edits, web search, apps, plugins, sub-agents). The only tools are the MCP tools you selected in the chat, offered as Codex dynamic tools and run by ReMa (with the same approvals as other providers). The thread uses a read-only sandbox in an empty folder, and Codex's own approvals are always declined.
- **Isolation.** The runtime is private to ReMa: its own `CODEX_HOME` under the app data folder, so your Codex CLI settings, sessions and MCP servers are neither read nor changed. It keeps no history.
- **Install.** Release builds of ReMa ship the latest Codex (see [Bundled runtimes](#bundled-runtimes)). ReMa uses the newest of the bundled and any installed copy (0.151 or newer).

**Anthropic: Claude Console account.** **Continue with Claude Console** runs `ant auth login`, from Anthropic's official CLI. Release builds ship it; it can also be installed with `brew install anthropics/tap/ant`.

- **Sign-in.** `ant` opens the Claude Console sign-in in your browser, receives the result on a local callback, and stores and refreshes the token.
- **Requests.** For requests, ReMa asks `ant auth print-credentials --access-token` for a short-lived token. This is the documented way to hand the credential to another program. ReMa keeps the token in memory for at most a minute and calls the Anthropic API with `Authorization: Bearer` and `anthropic-beta: oauth-2025-04-20`.
- **Isolation.** The CLI uses a ReMa-owned `ANTHROPIC_CONFIG_DIR`, so your own `ant` profiles are untouched.
- **Billing.** Usage is billed to your Console organization at API rates, like an API key.

**Claude Pro/Max (Claude.ai) sign-in is not offered.** Anthropic's documentation rules it out for apps like ReMa:

- The [Agent SDK overview](https://code.claude.com/docs/en/agent-sdk/overview) says: *"Unless previously approved, Anthropic does not allow third party developers to offer claude.ai login or rate limits for their products, including agents built on the Claude Agent SDK."*
- The [Legal and compliance page](https://code.claude.com/docs/en/legal-and-compliance) says: *"Anthropic does not permit third-party developers to offer Claude.ai login into their own applications, or to route requests through Free, Pro, or Max plan credentials on behalf of their users."*

So the Claude Agent SDK and Claude Code subscription sign-in are not used. If Anthropic approves ReMa, a runtime for it can be added in `accounts/` without changing chat or scheduler code.

**States and actions.**

- **States:** Disconnected, Connecting…, Opening browser…, Waiting for authorization…, Connected, Authentication expired, Authentication cancelled, Authentication failed, Reauthentication required.
- **Checks:** Settings checks each account connection with its runtime when it opens.
- **Actions:**
  - **Reconnect** signs in again.
  - **Change connection** switches between account and API key. Models are re-discovered, and an unused key is removed from the keychain.
  - **Disconnect** stops using the account; you stay signed in, so reconnecting is instant.
  - **Sign out** uses the runtime's own logout (`account/logout`, `ant auth logout`) and disconnects.

**Models** are always discovered per connection: Codex's `model/list` for a ChatGPT account, `/v1/models` for API keys and the Claude Console. A ChatGPT plan and an OpenAI API key can offer different models.

**Finding the runtimes.** Apps opened from the Finder don't inherit the terminal's `PATH`. ReMa therefore also looks in the usual install folders (Homebrew, npm, Volta, nvm, pnpm, Go) and asks your login shell. Set `REMA_CODEX_PATH` or `REMA_ANT_PATH` to use a specific executable. Debug builds also accept `REMA_ANTHROPIC_BASE_URL` / `REMA_OPENAI_BASE_URL` / `REMA_GEMINI_BASE_URL` for local mock servers.

## Google Workspace

**Connecting.** ReMa uses Google's official OAuth 2.0 flow for installed apps: authorization code with PKCE (S256), a random `state`, and a one-time loopback redirect (`http://127.0.0.1:<random port>`). The browser opens Google's consent page, and ReMa exchanges the code in Rust. The access and refresh tokens go to the OS keychain and are refreshed automatically. If Google revokes access, Settings shows "Reconnect needed". **Disconnect** revokes the grant at Google and deletes the tokens. Tokens never reach SQLite, config files, the frontend or a model.

**OAuth client.** Google requires every app to have its own OAuth client. Create one of type **Desktop app** in Google Cloud Console (APIs & Services → Credentials), enable the Gmail API and the Google Calendar API, and enter the client ID and secret in Settings → Google Workspace. The secret is stored in the keychain. A build can also embed a client with `REMA_GOOGLE_CLIENT_ID` / `REMA_GOOGLE_CLIENT_SECRET`.

**Scopes.** ReMa asks only for the services that are enabled:

- `gmail.readonly`: read-only mail access.
- `calendar.events`: read events to find conflicts, and create or update interview events.
- `openid email`: shows which account is connected.

**Job-application runs.** Each run works in this order:

1. Rust searches Gmail with a fixed query. The window is the lookback, extended back to the last successful run.
2. Rust skips emails that earlier runs already handled.
3. The model sees only the sender, subject and a short snippet of each new candidate, and picks the job-related ones.
4. Only those emails are read in full (truncated). The model returns JSON, and Rust validates it before storing anything.

Applications are matched deterministically, by Gmail thread, reference number, company plus role, or company domain, so repeated emails update one record.

**Calendar.** Only confirmed interviews become events, and only when the email itself states the date, start time, end time or duration, and time zone. The model must quote the email, and Rust checks the quote. Anything missing or ambiguous is marked Needs Your Action and nothing is written.

Events are idempotent. The event id is stored, and each event also carries a private `remaInterviewId` property. A content hash skips unchanged events.

- A reschedule updates the same event.
- A cancellation keeps the event, renamed "Cancelled: …" and marked free, and the change is recorded in the interview history.
- Events the user deleted are not recreated.
- Conflicts with busy events are reported. ReMa never moves an event.

## Profile

The Profile page has three independent parts, chosen with the tabs under its title. The selected part is part of the navigation state: uploading, previewing or re-rendering never switches it.

**Documents & Credentials** (default).

- **CVs.** Upload one or several files at once (PDF, Word `.docx`, text, Markdown). Each appears as a card with its type, size and date, and can be previewed, renamed, replaced, removed or marked as the **primary** CV. Files that cannot be added are listed with the reason. Nothing is copied into the Custom Profile.
- **Credentials.** Optional entries for degrees, professional and course certificates, training, licenses, badges and other evidence, each with an optional file (PDF, PNG, JPEG, WebP, Word) and optional details: title, issuer, issue and expiry dates, credential ID, URL and a note.
- **Preview.** Documents open inside ReMa: PDFs are drawn by PDF.js, images as images, and Word/text files as plain text blocks extracted by Rust (no HTML, scripts or macros ever reach the webview).
- **Storage rules.** The format is detected from the file's content (signatures), not only its extension; symbolic links and non-regular files are refused; files are limited to 20 MB; stored names are generated.

**Custom Profile** (optional). The structured form: personal details, professional title and summary, experience, education, skills, languages, links and custom fields. Empty fields are fine. **Fill from a CV…** is the only way to copy details from a document: Rust extracts the text, finds email addresses, phone numbers and links deterministically, and the default model structures the rest as JSON, which Rust validates (a name, email, phone or link that does not appear in the document is dropped). A review dialog shows every value; nothing is saved until you choose.

**Portfolio Studio.** CVs built in ReMa, separate from uploaded files (which are never changed).

- **Templates.** 12 designs in a registry (`src/lib/portfolio/templates.ts`): Minimal, Modern, Executive, Technical, Data / AI, Consulting, Product, Creative, Academic, Compact, Two-column and ATS-friendly. A template is data read by one layout engine (`src/lib/portfolio/layout.ts`), so adding one means adding an entry.
- **Editor.** Header and sections (summary, experience, projects, education, skills, languages, certifications, links, custom): add, remove, reorder, show or hide, rename. Switch templates, paper size (A4, Letter) and accent color at any time without losing content. Changes save automatically.
- **Preview and export.** pdfmake renders the PDF in the interface with embedded open-license fonts (Inter, Source Sans 3, Source Serif 4, Source Code Pro, Playfair Display); the live preview shows that same file through PDF.js, and **Export PDF** saves it where you choose (the save dialog runs in Rust). Text is real and selectable, margins follow the template, and long entries flow onto further pages without being cut.
- **Start.** A new CV starts with empty sections or a one-time copy of the Custom Profile and credentials.

## Built-in browser

The browser is a second Tauri webview (label `browser`) inside the main window. It is not a page in the navigation: it opens beside the current page when you click a link, or from the globe button in the header.

**Isolation.**

- ReMa's commands are ACL-checked through an app manifest (`src-tauri/build.rs` + `src-tauri/src/ipc_commands.rs`). They are granted only to the `main` webview on ReMa's own origin (`capabilities/default.json`). Remote pages get no capability, so every command and event listener is refused. This includes `get_profile`, the Google and chat commands, and the opener.
- ReMa injects no Profile data and no bridge into pages. The only initialization script keeps `target="_blank"` links in the same view.
- A navigation policy in Rust allows `http(s)` pages but blocks ReMa's own origins, `file:`, `tauri:`, `data:`, `javascript:` and custom schemes. `mailto:` and `tel:` go to the system.
- Cookies and sessions persist like a normal browser profile. ReMa never reads them or sends them to a model. CAPTCHA and sign-in pages are left to you.

If a site needs pop-ups (for example some "Sign in with…" flows) or otherwise misbehaves, use **Open in external browser**.

## ReMa Auto Fill

Auto Fill runs only when you click **ReMa Auto Fill** in the browser toolbar:

1. A scan script lists the page's form fields: name, id, autocomplete, label, placeholder, type, options and nearby text. It reports only whether a field is empty, never its value.
2. Rust classifies each field deterministically. It checks `autocomplete` first, then the field's own label, placeholder, name and id, and falls back to nearby text only for unlabeled fields. No model is used.
3. Rust sends back values for the matched fields only. They go into **empty** fields through the native value setter and input/change events, so pages see normal typing. Filled values stay editable.
4. Passwords, checkboxes, radio buttons, consent boxes and open questions ("Why do you want…", salary expectations) are left for you and listed in a report.
5. For CV/resume file inputs, you pick a Profile document and ReMa attaches it. If the page refuses a programmatic file, **Show file** reveals it so you can choose it in the page's own file picker.

Auto Fill never clicks buttons and never submits a form. Forms inside cross-origin frames are reported, with a link to open them directly.

## Profile context

The **Profile** switch in the chat composer is the single control for career context. When it is on (it is saved per conversation), or when a scheduled prompt task has **Include my Profile** checked, the context builder (`services/profile_context.rs`) assembles the available sources locally, in this order:

1. the primary CV's extracted text,
2. the Custom Profile (structured fields),
3. credentials (type, title, issuer, dates, ID, URL, note),
4. Portfolio Studio CVs (visible sections only),
5. other CVs, then the names of other documents.

Empty sources are skipped. Each source is wrapped with its type and name, so the model (and ReMa) know where every part came from. Email addresses and phone numbers are removed. The whole context is limited to about 24,000 characters, with per-source limits; when it must be shortened, the primary CV and structured data keep their share and long texts are cut at line boundaries. Extracted text is cached in SQLite when a document is stored, so nothing is re-read or uploaded in the background. Nothing is sent until you send a message, and with the switch off no Profile data is sent.

## Agents

An agent is a named, locally stored set of instructions. The **Agents** page (below Chat) lists the built-in agents and your own:

- Built-in agents are fixed; **Customize a copy** creates an editable custom agent.
- Custom agents can be created, edited, renamed, duplicated and deleted. Deleting one removes it from every chat that used it.
- **Use in chat** opens a new chat with the agent selected.

In Chat, the **+** menu selects any number of agents (up to 8). Their instructions are added to the system prompt once each, in the order you selected them. Selecting an agent never turns the Profile switch on. Selections are saved per conversation.

## MCP (Model Context Protocol)

**Settings → MCP** manages MCP servers. The client (`src-tauri/src/mcp/`) uses the official Rust SDK (`rmcp`) and speaks the current protocol (2026-07-28, stateless `server/discover`) with a fallback for servers on 2025-11-25.

- **Transports.** *Local program* (stdio): a program and its arguments, run directly (never through a shell). Shells, relative paths and shell syntax are refused; arguments are passed one per line exactly as written; the process gets only a minimal environment plus the variables you set; it runs in its own process group and stops with the connection. *Remote server* (Streamable HTTP): an HTTPS URL (plain HTTP only for this computer) with no authentication, a bearer token, an API-key header, or **OAuth** sign-in in the browser (MCP authorization with a loopback redirect).
- **Secrets** (tokens, header values, environment values, OAuth credentials) are stored in the OS credential store and never shown again or sent to the interface; leaving a field empty keeps the stored value.
- **States.** A new server is **off**. *Configured* → *turned on* in Settings → *available* in the chat's + menu → *selected* in a chat → a tool *called* by the model. Only servers that are on appear in the + menu, and only the ones selected in a chat are offered to the model in that chat.
- **Approvals.** Tools the server marks read-only run directly; every other call shows its arguments and waits for **Allow once**, **Allow for this chat** or **Deny**. Stopping the answer denies what is still waiting.
- **Turning a server off** closes its connection (and stops a local program). Chats that selected it drop it with a short note; turning it on again does not re-add it.
- **Test**, **Connect/Reconnect**, **Disconnect**, **Sign in/Sign out**, **Edit** and **Remove** are on each server row; connection failures show the server's reason.

## Bundled runtimes

Release builds include the official **Codex** runtime (latest `@openai/codex` from npm) and Anthropic's **`ant`** CLI (from Anthropic's Homebrew tap), so the account sign-ins work without installing anything. `pnpm build:app` downloads the current versions for the target platform (checking their published SHA-512 / SHA-256), then builds the app with them as sidecars (`src-tauri/tauri.runtimes.conf.json`). At run time ReMa uses the newest of the bundled and any installed copy. `pnpm tauri dev` and `pnpm tauri build` work without them.

## Analytics

**Opening it.** The **Analytics** button in the header opens a panel at half the width of the workspace, beside the current page. It is not a page in the navigation. The panel can be resized, collapsed to a strip, maximized, minimized to the header button, and closed. It reopens as you left it: size and mode are kept in the window, and the scope, filters, ranking, columns, tab and options are saved in SQLite. **Analyze** on a chat answer or task result opens it with that search selected. A job row opens the job in the built-in browser, and Analytics and the browser can sit side by side.

**Where the jobs come from.** Every chat answer and prompt-task result that contains a job table is read automatically when it finishes, and older history is read once at start. (The chat system prompt asks the model to list jobs as a table with company, role, location, work mode, salary, posting date, key skills and link.) Answers that list jobs as text can be read with **Analyze**, which parses common list formats and then asks the model; the model's values must appear verbatim in the answer. Each answer or run becomes a *job search run*, which keeps its own list of jobs even when those jobs are shared with other runs.

**Normalization.** Rust normalizes each job: role (seniority words removed), seniority, work mode, employment type, cities and countries, salary (minimum, maximum, currency, period) and posting date. A value the source does not state stays empty. ReMa never guesses a salary, seniority, location, date or requirement. Estimated salaries are ignored. Relative dates ("3 days ago") count from when the job was found, and the date it was found is never used as a posting date. Ambiguous dates such as `03/04/2026` are not guessed.

**Deduplication.** A job is the same job when it shares any of these keys (checked in this order):

1. the job board's own id (LinkedIn, Indeed, Greenhouse, Lever, Workday, Personio, StepStone, Glassdoor, SmartRecruiters, Ashby, karriere.at, XING);
2. the canonical URL, without tracking parameters (when the URL points to one job);
3. the company's own reference number;
4. a fingerprint of the job description;
5. company + normalized title + location.

A job listed as "Austria" and the same job listed as "Vienna, Austria" also match, but two jobs in different cities do not. Merged jobs keep every run they appeared in, so "Analyze" on one search still shows that search's jobs.

**Job details.** A background worker reads each job's own page, one at a time: it follows only public addresses (never this computer or the local network, also after redirects), reads at most 3 MB, stops at pages that refuse it (401, 403, 429), and prefers the page's `JobPosting` structured data. Requirements are then extracted deterministically with a skill dictionary and aliases (K8s → Kubernetes, Postgres → PostgreSQL). Optionally, the default model extracts them from the description as JSON; every item must quote the description, and Rust rejects anything it cannot find there. Both options can be turned off in the scope editor. In debug builds, `REMA_DEV_ALLOW_LOCAL_PAGES=1` lets the worker read pages from a local test server.

**Filters and ranking.** Filters are typed conditions (country, city, work mode, role, company, seniority, salary, posting date, skill, language, certification, Profile match, missing skill, search…), combined with AND; a condition with several values matches any of them. They run in Rust. Ranking is an ordered list of criteria (for example highest salary, then preferred country, then newest), each with its own direction. Jobs without a value always rank last. There is no hidden score. "Analyze only the top N" limits the analysis to the best-ranked jobs.

**Salary.** Salaries are compared only per year and only in one currency: yearly and monthly amounts are converted to yearly, hourly and daily rates are not compared, and jobs in other currencies are listed but not compared (the dashboard says so). The midpoint of a range is used. A missing salary is never zero.

**Skill gap.** Each requirement of each job is compared with the Profile: **Matched**, **Partial** (a related skill, fewer years, a lower language level or degree), **Missing**, or **Unknown** (for example soft skills, which a Profile cannot prove). Unknown requirements never count as gaps. Coverage = (matched + ½ partial) ÷ (matched + partial + missing). Gap priority = share of jobs weighted by importance (required 1, preferred 0.5) and gap (missing 1, partial 0.5), optionally weighted by rank. It is raised a little when the skill is more common among the better-paid jobs, and only when at least 5 salaries can be compared. Charts: demand frequency, coverage vs demand, a job × skill matrix, and gap priority; each has a table view. Without a Profile, the dashboard says "Profile required for personal skill-gap comparison." and shows demand only.

**Requirements.** Every normalized requirement with its category, number of jobs and share of jobs, classed as very common (≥ 70%), common (40–70%), occasional (15–40%) or rare (< 15%, only with 5 or more jobs). The table can be searched, filtered by category, class or Profile state, and sorted, and selected requirements can become filters.

**Learning.** Rust picks the gaps to learn from the deterministic priorities and your criteria (minimum share, ignore single-job skills, include partial matches, maximum number of gaps). **Research learning resources** sends only those gaps to the default model and asks for resources as JSON. Rust validates it: the gaps must be the ones it sent, links must be public `http(s)` URLs, and each link is checked once for reachability. The explanation of why each gap matters is written by Rust from the jobs themselves. Research is stored with the date, dataset and gap snapshot, is marked outdated when the data changes, and runs again only when you click **Refresh**.

## Scheduler behaviour

- Tasks run only while ReMa is open. If ReMa was closed through one or more scheduled times, the task runs **once** at the next start and then continues on schedule.
- A run is claimed and the schedule advanced *before* it starts, so it can never run twice. A task that is still running skips its next occurrence.
- "Run now" does not change the schedule or count toward "stop after N runs".
- Daily and weekly schedules keep wall-clock time in the task's timezone, including across daylight-saving changes. Intervals are exact durations.
- The 15-minute minimum is enforced in Rust (the frontend only mirrors it).
- Each run is limited to 15 minutes. Failures are recorded in the history without breaking the task.

## Prerequisites

- **Rust** (stable, 1.88+): <https://rustup.rs>
- **Node.js** 20.19+ or 22.12+
- **pnpm** 10: `corepack enable`
- **Platform dependencies for Tauri**: <https://v2.tauri.app/start/prerequisites/>
  - macOS: Xcode Command Line Tools (`xcode-select --install`)
  - Linux (Debian/Ubuntu): `sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev` plus a Secret Service provider (for example GNOME Keyring or KWallet) for API keys.

## Getting started

```bash
pnpm install
pnpm tauri dev      # launches the desktop app with hot reload
```

Then open **Settings**, connect a provider (or add an OpenAI-compatible endpoint such as `http://localhost:11434/v1` for Ollama), and start chatting.

## Scripts

| Command                  | What it does                                              |
| ------------------------ | --------------------------------------------------------- |
| `pnpm tauri dev`         | Run the desktop app in development mode                   |
| `pnpm tauri build`       | Build an optimized, bundled release of the app            |
| `pnpm build:app`         | Same, with the latest Codex and `ant` runtimes bundled    |
| `pnpm runtimes`          | Download the latest Codex and `ant` into src-tauri/runtimes |
| `pnpm build`             | Typecheck and build the frontend only                     |
| `pnpm typecheck`         | Typecheck the frontend                                    |
| `pnpm lint`              | Lint the frontend with oxlint                             |
| `pnpm test`              | Run the frontend tests (Vitest, incl. real PDF rendering) |
| `pnpm test:rust`         | Run the Rust tests (fails if bindings are stale)          |
| `pnpm bindings`          | Regenerate `src/generated/bindings.ts` from Rust          |
| `pnpm brand`             | Regenerate the logo files from the master mark            |
| `pnpm icons`             | Regenerate the desktop app icons from the app icon SVG    |

## Project structure

```text
rema/
├── src/                          # Frontend (React + TypeScript)
│   ├── app/                      # App root, navigation, sidebar entries
│   ├── pages/                    # ChatPage, AgentsPage, ScheduledTasksPage, ProfilePage, SettingsPage
│   ├── assets/brand/             # Master mark (mark.json) and the generated logo variants
│   ├── components/
│   │   ├── layout/               # AppShell, Sidebar, PageContainer, ThemeToggle, LaunchIntro
│   │   ├── browser/              # BrowserProvider, BrowserPanel, Auto Fill button and report
│   │   ├── analytics/            # AnalyticsPanel, scope/filter/ranking editors, Jobs, Skill gap,
│   │   │                         # Requirements and Learning tabs, charts, Analyze button
│   │   ├── chat/                 # Composer, + menu and chips, tool activity and approvals, MessageList, …
│   │   ├── agents/               # Agent icons, create/edit dialog
│   │   ├── profile/              # Profile tabs: documents & credentials (cards, viewer), Custom Profile
│   │   ├── portfolio/            # Portfolio Studio: gallery, new CV, editor, section editor
│   │   ├── pdf/                  # PDF.js page renderer (viewer and preview)
│   │   ├── tasks/                # TaskDialog, TaskDetail, JobReport, TaskActions, TaskStatus
│   │   ├── settings/             # Provider connections (account or key), endpoints, MCP servers, Google
│   │   └── ui/                   # Menu, Dialog, Switch, EmptyState, IconButton, StatusIndicator,
│   │                             # BrandMark, BrandLogo
│   ├── hooks/                    # useAsyncData, useChat, useTasks, useProfile, useAutofill, …
│   ├── services/                 # ipc.ts (callBackend, ApiError), events.ts, one service per area
│   ├── generated/bindings.ts     # Generated from Rust (do not edit)
│   ├── lib/                      # markdown.ts (safe rendering), format.ts, taskForm.ts, profileMerge.ts, analytics.ts,
│   │                             # theme.ts, chatSelection.ts, documents.ts, pdf/ (PDF.js setup),
│   │                             # portfolio/ (templates, layout engine, fonts, PDF rendering)
│   └── styles/                   # tokens → base → layout → components → chat/tasks/settings/profile/browser/analytics
├── public/theme-init.js          # Applies the saved theme before the first paint
├── scripts/brand/                # Logo generator (build-brand.mjs), app icons (app-icons.mjs), outlined wordmark
├── scripts/runtimes/fetch.mjs    # Downloads the latest Codex and ant for bundling
│
└── src-tauri/src/                # Backend (Rust)
    ├── lib.rs                    # Startup: database, keychain, scheduler, IPC
    ├── ipc.rs                    # Command/event registration + bindings export
    ├── ipc_commands.rs           # Command list for the permission manifest (checked by a test)
    ├── commands/                 # Thin Tauri commands
    ├── services/                 # chat, providers, tasks, scheduler, schedule (pure math), system,
    │                             # profile, documents (storage + text extraction), profile_import, profile_context,
    │                             # portfolio, agents, mcp, chat_tools (MCP tools in chat, approvals)
    ├── mcp/                      # MCP client: config validation, connections (stdio/HTTP), OAuth
    ├── protocol.rs               # rema-doc:// document files for the main window
    ├── browser/                  # Built-in browser: webview, navigation policy, Linux embedding, autofill
    ├── analytics/                # Job analytics: table/list parsing, normalize, skills dictionary, ingest
    │                             # (dedupe), page reader + background worker, dataset, filter, rank,
    │                             # matching, overview, gap, unique (requirements), learning
    ├── jobs/                     # Job-application workflow: extract, interviews, applications, calendar_sync, report
    ├── accounts/                 # Provider account sign-in: Codex app-server client, Anthropic CLI,
    │                             # runtime discovery, sign-in sessions
    ├── integrations/google/      # OAuth (PKCE, refresh, revoke), Gmail and Calendar tools
    ├── llm/                      # LanguageModel trait, SSE, HTTP, OpenAI/Anthropic/Gemini adapters
    ├── db/                       # SQLite connection, migrations, repositories
    ├── secrets.rs                # OS credential store
    ├── events.rs                 # Backend → frontend events
    ├── models/                   # Typed data shared with the frontend
    ├── state.rs                  # Shared AppState
    └── error.rs                  # AppError → { code, message }
```

## Adding a backend command

1. Add types to `src-tauri/src/models/` (derive `Serialize` and `specta::Type`) and logic to `src-tauri/src/services/`, with tests.
2. Add a thin `async` command in `src-tauri/src/commands/` (`#[tauri::command]` + `#[specta::specta]`, returning `AppResult<T>`).
3. Register it in `collect_commands![...]` in `src-tauri/src/ipc.rs` and add its name to `APP_COMMANDS` in `src-tauri/src/ipc_commands.rs` (a test checks both lists match; unlisted commands are refused at runtime). Then run `pnpm bindings`.
4. Wrap it in a frontend service (`callBackend(() => commands.myCommand(...))`) and use it from a hook.

## Window chrome

On macOS the window uses Tauri's overlay title bar, so the traffic lights sit inside ReMa's header. The header is the window drag area; buttons placed in it stay clickable. Windows and Linux keep their native title bars.

## Design system

The look is defined once, in `src/styles/tokens.css`, and every stylesheet uses those tokens:

- **Color**: LinkedIn blue (`#0A66C2`, hover `#004182`) for actions, selection and focus; white surfaces on a warm light-gray canvas (`#F4F4F2`); charcoal text. Secondary and meta text meet WCAG AA contrast on both surfaces. Status colors (success, warning, danger) always come with an icon or a label. Two blue roles: `--color-brand` fills controls (white text on it) and `--color-accent` is blue text, icons and outlines; they are the same in light and differ in dark.
- **Themes**: light is the default. Dark (`:root[data-theme='dark']` in `tokens.css`) is a calm, warm charcoal in the spirit of Claude's desktop app: a `#262624` canvas, `#2E2D2B` cards and a `#1F1E1D` title bar and rail, quiet borders, deeper but restrained shadows, warm off-white text (12.7:1; secondary 8.4:1), and ReMa blue only for actions, selection and accents (`#5BA3EF` for blue text, 5.7:1). The sun/moon button (`ThemeToggle`) switches it. The choice is kept in two places: the webview's storage, read by `public/theme-init.js` before the first paint so there is no flash, and ReMa's settings table (`set_appearance`), from which Rust sets the native window theme and background at startup. Charts redraw in the new colors; checkboxes, radios, selects, scrollbars and text selection follow the theme.
- **Composer glow**: a slow, soft blue aurora behind the chat composer (`.composer-aurora` in `chat.css`). It stays within about 40 px of the composer's edges, never covers the page, and drifts with transform-only animations of 26–38 s; it brightens slightly while the composer has focus. It is still under "Reduce motion".
- **Type**: the native system font (SF Pro on macOS, Segoe UI on Windows), with Inter bundled for other platforms (`@fontsource-variable/inter`). The scale: 28px page titles, 15px section titles, 14px body, 13px secondary text and controls, 12px meta, and 11px monospace uppercase eyebrows, a nod to the ReMa website.
- **Space and shape**: a 4px grid; 28, 32 and 36px controls; 8px radius for inputs, 12px for cards, 16px for dialogs; pill buttons.
- **Depth**: hairline borders carry separation; shadows stay light and are stronger only for popovers and dialogs.
- **Motion**: 110–220ms with ease-out curves, used for hover and press states, popovers, dialogs, panels and page changes. Everything is off with "Reduce motion".
- **Sidebar**: the button next to the ReMa wordmark (or ⌘B on macOS, Ctrl+B elsewhere) collapses the sidebar to an icon rail with Chat, Scheduled Tasks, Profile, New chat and Settings; ReMa remembers the choice on this computer.
- **Launch intro**: each start of ReMa opens with "ReMa — Your Career Agent" on the theme's canvas, warm white or warm charcoal (about 3 seconds, `src/components/layout/LaunchIntro.tsx`), while the app loads underneath. It plays once per launch (in-memory, nothing is stored); any key or click skips it.
- **Desktop conventions**: buttons keep the arrow cursor (only text links show the hand), and there is one focus ring for keyboard users everywhere. Thin scrollbars appear outside macOS, which keeps its native overlay scrollbars.

Shared building blocks live in `src/styles/components.css` (buttons, inputs and styled native selects, checkboxes and radios, switches, badges, menus, dialogs, data tables, empty and loading states) and `src/components/ui/` (`Dialog`, `Menu`, `Switch`, `EmptyState`, `StatusIndicator`, `IconButton`, `BrandMark`, `BrandLogo`).

## Brand

The ReMa mark is one abstract symbol: an R drawn as two forms that meet at a node.

- **The loop** (stem and bowl) is where the search starts: you and your Profile.
- **The path** (the leg) leaves it toward the lower right: the way forward, the next role.
- **The node**, held in a small clearing where the two meet: the match.

The geometry is defined once, in `src/assets/brand/mark.json`: two round-capped strokes and a node on a 120 × 120 grid, with no fine detail, so the silhouette holds at 16 px. Every variant is generated from it; only color, lighting and scale change.

| Variant | For | Files (`src/assets/brand/`) and in-app use |
| --- | --- | --- |
| A. App icon | macOS, Windows and Linux app icon, Dock, taskbar, launcher, website hero | `rema-app-icon.svg` → `src-tauri/icons/` |
| B. Navigation | 16–32 px: title bar, rails, compact UI. Flat, two tones, no effects | `rema-nav-light.svg`, `rema-nav-dark.svg`; `<BrandMark />` |
| C. Logo + wordmark | Website header, About, onboarding, marketing | `rema-logo-light.svg`, `rema-logo-dark.svg`; `<BrandLogo />` (Settings → About) |
| D. Monochrome | Print, overlays, system integrations, high-contrast contexts | `rema-mark-white.svg`, `rema-mark-black.svg`, `rema-mark-blue.svg` |
| E. Dark mode | Dark surfaces: brighter blues, edge light, a controlled glow | `rema-mark-dark.svg`; `<BrandMark variant="full" />` in dark |
| F. Light mode | Light surfaces: deeper blues, crisp edges, no glow | `rema-mark-light.svg`; `<BrandMark variant="full" />` in light |

- **Color and finish**: the ReMa blue family, from deep blue through cobalt to a cyan-leaning path and a light node, with a soft top highlight. The app icon sets a white-to-ice mark on a blue tile (cobalt `#2A8CF0` → LinkedIn blue `#0A66C2` → deep `#062F66`) with a soft internal light, a thin rim light and a faint cyan glow. It is not metallic or neon.
- **App icon shape**: a rounded square on the macOS icon grid (an 824 px tile on a 1024 px canvas, transparent corners), so it sits with other apps in the Dock and the Windows taskbar.
- **Wordmark**: "ReMa" in Inter (weight 650, −2% tracking), outlined to paths (`scripts/brand/wordmark.json`) so it looks the same everywhere.
- **Changing the mark**: edit `mark.json`, then run `pnpm brand` (writes the SVG variants) and `pnpm icons` (renders the app icon into `src-tauri/icons/`). The in-app `BrandMark` reads `mark.json` directly, in the current theme's colors.
