# ReMa

**Career UI Harness**: a fast, lightweight desktop app built on a deterministic Rust core with an embedded LLM layer.

Rust owns application state, persistence, scheduling, validation, provider configuration and execution. LLMs are used only to reason about prompts and write the responses.

## What ReMa does

- **Chat**: a general-purpose AI chat with streaming responses, Stop, Retry, Copy, Markdown (code, lists, tables, links) and persistent conversation history ("Recents"). Job searches always search the web first: ReMa validates the postings it finds and shows only those, with direct links, before the model comments (see [Career search](#career-search)).
- **Models**: OpenAI, Anthropic, Google Gemini, and any OpenAI-compatible endpoint (Ollama, LM Studio, vLLM, …). Connect OpenAI with your **ChatGPT account** and Anthropic with your **Claude Console account** in the browser, or use API keys. Pick the model in the chat composer.
- **Scheduled Tasks**: schedule the prompt you are writing, straight from the composer. Supports one-time, daily, every N days, selected weekdays, and intervals of 15 minutes or more. A task can end on a date or after N runs. You can edit, pause, resume, delete or run tasks now. Click a task to open **Scheduled / <Task>**: every run (scheduled or Run now) with its own result, progress, the context it used, its outputs and the task as it was when it ran; a running task updates live.
- **Settings**: connect providers, choose which models appear in the chat, and set the default model. **Connectors** add Gmail, Google Calendar, Outlook Mail and Outlook Calendar with a sign-in in your browser (see [Connectors](#connectors)).
- **Applications**: every job application ReMa tracks from your job mail, in five sections: Interviews Confirmed, Applications Confirmed, Needs Your Action, Applications In Progress and Rejected. Each row shows the date of the latest email, company, role, status and the latest update, the requested action or the rejection reason. An application also has a timeline that says where every change came from ("Application changed to Interview · Source: Gmail message · Confidence: 98%"), its interviews and its emails (see [Applications](#applications)).
- **Job Mail & Interview Sync**: a built-in scheduled task, and the only part of ReMa that reads your mail. Once you turn it on, it reads the last N days (30 by default), then only new mail, keeps Applications up to date, and adds confirmed interviews to your calendar when the time is free. Notifications appear in the bell at the top and from the operating system.
- **Calendar**: the calendar button at the top right shows your connected Google and Outlook calendars inside ReMa, a week at a time, with your interviews marked and their meeting links one click away.
- **Profile**: your career context, in two parts. **Documents & Credentials** (the default) keeps your CV files and optional credentials (degrees, certificates, courses, licenses, badges) with an in-app preview. **Custom Profile** is an optional structured form for your current goals, preferences and facts. Uploading a CV never fills the Custom Profile.
- **Portfolio Studio**: its own page below Profile. Builds new CVs from 12 templates and exports them as PDF.
- **Agents**: reusable instructions for the model (a page directly below Chat). Six built-in agents (Job Search, Job Match Analyst, CV Tailoring, Interview Prep, Application Strategist, Career Research) and your own custom agents.
- **MCP (Model Context Protocol)**: connect local or remote MCP servers in Settings, turn them on, and pick them per chat. The model can then call their tools; tools that may change something wait for your approval.
- **Built-in browser**: links in Chat and task results open in a panel next to ReMa. You can collapse, maximize, dock left or right, and resize it. "Open in external browser" is always available.
- **ReMa Auto Fill**: fills the standard fields of an application form in the built-in browser from your Profile. You review and submit yourself. ReMa never submits.
- **Chat + menu**: the **+** button in the composer selects agents and MCP servers for the chat (shown as removable chips), independently of the **Profile** switch.
- **Profile context**: the **Profile** switch in the chat composer (and an option on scheduled prompt tasks) combines your Custom Profile, CVs and credentials into one career context for each message. The Custom Profile wins when it disagrees with a document. It is off unless you turn it on.
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
                                                      portfolio, agents, mcp, mail_monitor, calendar_view)
                            src-tauri/src/mcp        (MCP client on the official Rust SDK: stdio, Streamable HTTP, OAuth)
                            src-tauri/src/jobs       (job-application workflow, run by the scheduler)
                            src-tauri/src/browser    (built-in browser webview, navigation policy, Auto Fill)
                            src-tauri/src/analytics  (job ingestion, dedupe, filters, ranking, skill gap, learning)
      ↓
Persistence / providers     src-tauri/src/db (SQLite) · src-tauri/src/llm (adapters) · src-tauri/src/secrets (OS keychain)
Provider accounts           src-tauri/src/accounts (official local runtimes: Codex app-server, Anthropic CLI)
Connectors                  src-tauri/src/connectors (OAuth + PKCE, token manager, Gmail, Google Calendar,
                                                      Outlook mail and calendar via Microsoft Graph, sync)
```

Rules:

- **Deterministic core.** The scheduler decides when tasks run, using pure, unit-tested schedule math. The LLM never controls timing, state, credentials or configuration.
- **Provider-neutral.** Chat and scheduler use one `LanguageModel` interface. Provider, transport, authentication and model are separate concepts: the HTTPS adapters (`llm/openai_responses.rs` for OpenAI's Responses API, `llm/anthropic.rs`, `llm/gemini.rs`, and `llm/openai.rs` for every OpenAI-compatible endpoint's Chat Completions) and the Codex runtime (`accounts/codex.rs`) are transports; a connection method says how a request is authenticated; models are discovered per connection. Web search is each provider's hosted tool, reported back as `WebEvent`s; job and research requests go through `career_search/` (ReMa's no-key sources and the model's own search at once → validate → answer), enforced in Rust.
- **Rust is the source of truth.** IPC types, commands and events are generated from Rust. The frontend never hand-writes copies of them.
- **Streaming via events.** Replies stream as typed `ChatEvent`s. Generation runs in the background, so switching chats does not interrupt it.

How a model call behaves (`src-tauri/src/llm/mod.rs`, `services/chat.rs`; audit record: [docs/audit/implementation.md](docs/audit/implementation.md)):

- **History.** A chat sends its newest turns that fit a budget (about 200,000 characters for hosted models, 16,000 for OpenAI-compatible endpoints, which often run small context windows). Older turns are left out, and the model is told so.
- **Tool loop.** A model may call tools in up to 8 rounds (and resume a provider's `pause_turn` up to 4 times). After the 8th round it is asked once more with tools off (`tool_choice: none`, Gemini `functionCallingConfig: NONE`), so the answer is kept instead of lost. Calls a provider sends without ids get ids no other call in the answer has. Each round sends the model's turn back as it came: Anthropic content blocks, Gemini parts (thought signatures, search calls and results), OpenAI reasoning items (`encrypted_content`).
- **Temporary errors.** 408, 429, 5xx and 529 answers, and failed connections, are asked again twice before anything streamed (1 s, then 3 s, or the provider's `retry-after` up to 20 s). An empty quota is never asked again.
- **Output limit.** An answer cut off at the model's output limit keeps its text and ends with a note that it was cut off, in chat and in scheduled tasks. Job Mail & Interview Sync treats a cut-off reply as failed and retries it in the next run. Claude 5-series models think by default, so their small internal requests get room for thinking on top of the answer.
- **Models without tool support.** When an endpoint refuses tools ("does not support tools"), ReMa asks again with no tools and no web access, with a system prompt that says so, and remembers it for that model while ReMa runs. Scheduled prompt tasks do the same.
- **Inline reasoning.** `<think>…</think>` text that some local models put into their answer is left out of the reply.
- **Credential health.** When a provider rejects its key or sign-in (401/403, or Google's `API_KEY_INVALID`), Settings shows the provider as needing attention (**Replace key** for API keys, **Reconnect** for accounts; custom endpoints say to edit the key) until a request succeeds. A rejected API key stays marked after a restart (only the message is stored, never the key).

## Data and credentials

- **Database**: SQLite at `<app data dir>/rema.db`. On macOS that is `~/Library/Application Support/cloud.datasound.rema/`. Schema changes are versioned migrations in `src-tauri/src/db/migrations/`.
- **Profile documents**: copied into `<app data dir>/profile-documents/` under generated names. The original file name, size, SHA-256 and extracted text are stored in SQLite. The original file is never changed or moved. The interface reads a stored file only through the `rema-doc` protocol (by id, ReMa's own window only), never by path.
- **Portfolio Studio CVs, custom agents and MCP server settings** are stored in SQLite. MCP secrets (tokens, header values, environment variable values, OAuth credentials) are in the OS credential store, never in the database or the interface.
- **Credentials**: API keys and connector OAuth tokens (Google, Microsoft) are stored in the operating system's credential store: macOS Keychain, Windows Credential Manager, or Secret Service on Linux. They never go in the database, config files or the frontend. The UI can only save, replace or remove a key.
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
- **Chat.** Each chat turn runs on an ephemeral thread. ReMa's system prompt replaces Codex's instructions and earlier messages are added as history. Codex's web search is turned on for chat threads (`config: {"web_search": "live"}`, or the best mode the workspace allows; OpenAI runs the searches). If the account may not search, the answer says so, and a job search uses ReMa's own sources instead of answering unverified. Every local tool is off (shell, file edits, apps, plugins, sub-agents). The only other tools are the MCP tools you selected in the chat, offered as Codex dynamic tools and run by ReMa (with the same approvals as other providers). The thread uses a read-only sandbox in an empty folder, and Codex's own approvals are always declined.
- **Isolation.** The runtime is private to ReMa: its own `CODEX_HOME` under the app data folder, so your Codex CLI settings, sessions and MCP servers are neither read nor changed. It keeps no history.
- **Install.** Release builds of ReMa ship the latest Codex (see [Bundled runtimes](#bundled-runtimes)). ReMa uses the newest of the bundled and any installed copy (0.151 or newer).

**Anthropic: Claude Console account.** **Continue with Claude Console** runs `ant auth login`, from Anthropic's official CLI. Release builds ship it; it can also be installed with `brew install anthropics/tap/ant`.

- **Sign-in.** `ant` opens the Claude Console sign-in in your browser, receives the result on a local callback, and stores and refreshes the token.
- **Requests.** For requests, ReMa asks `ant auth print-credentials --access-token` for a short-lived token. This is the documented way to hand the credential to another program. ReMa keeps the token in memory for at most a minute and calls the Anthropic API with `Authorization: Bearer` and `anthropic-beta: oauth-2025-04-20`.
- **Isolation.** The CLI uses a ReMa-owned `ANTHROPIC_CONFIG_DIR`, so your own `ant` profiles are untouched.
- **Billing.** Usage is billed to your Console organization's prepaid API credits, like an API key. It is separate from a Claude Pro or Max plan: with no credits left, every request fails with "credit balance is too low". ReMa then says so in the chat (with a link to the Console's billing page) and shows **No API credits left · Add credits** on the Anthropic row in Settings until a request succeeds.

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

## Connectors

Settings → Connectors has four cards: **Gmail** and **Google Calendar** (by Google), **Outlook Mail** and **Outlook Calendar** (by Microsoft). `+` connects, a green check shows a working connector, and a card that needs attention says why (reconnect, missing permission, failed sync) with technical details on request. A card opens its details: account, permissions, last sync, whether Job Mail & Interview Sync is on, **Reconnect** and **Disconnect** (after confirmation). Connecting reads no mail: only [Job Mail & Interview Sync](#job-mail--interview-sync) does.

**Signing in.** ReMa uses OAuth 2.0 for native apps (RFC 8252): authorization code with PKCE (S256), a random `state` checked on return, a one-time loopback redirect (`http://127.0.0.1:<port>` for Google, `http://localhost:<port>` for Microsoft) and your default browser, never an embedded one. ReMa brings its own app registrations (a Google "Desktop app" client and a Microsoft Entra public client, set when ReMa is built; see [docs/connectors/registration.md](docs/connectors/registration.md)); users never create or enter OAuth clients. Installed apps get no incremental authorization, so a Google sign-in asks for the union of the enabled Google connectors, and ReMa checks which permissions were actually granted.

**Permissions** (least privilege, no send or modify scope):

| Connector | Google | Microsoft Graph (delegated) |
|---|---|---|
| Identity | `openid`, `email`, `profile` | `openid`, `profile`, `email`, `offline_access`, `User.Read` |
| Mail | `gmail.readonly` | `Mail.Read` |
| Calendar | `calendar.events`, `calendar.freebusy` | `Calendars.ReadWrite` |

**Tokens.** Access and refresh tokens live only in the OS credential store and in Rust memory: never in SQLite, settings, logs, the interface or a model's context. Access tokens are refreshed silently five minutes before they expire (Microsoft refresh tokens rotate). **Staying signed in**, as in Claude and ChatGPT: you connect once. While ReMa runs it renews each connected account at start and once a day (a token refresh only; nothing is read), so a grant that would lapse when unused (Microsoft after a while unused, Google after about six months) stays alive. One renewal runs at a time per account; passing trouble (no connection, a timeout, a 429 or 5xx) is retried a few times and never ends a connection, and a renewal still in flight when you disconnect saves nothing. Google ends every sign-in about 7 days after it was made while the Google app is in Testing: that is a setting of the app (Google Auth Platform → Audience → Publish app), not something ReMa can renew around. A build made with `GOOGLE_PUBLISHING_STATUS=testing` says on its Google cards when Google is expected to end the sign-in (an estimate). When a provider refuses a renewal, the card says what the provider said and whose setting it is: Google says only that the sign-in ended (a revoked grant, a changed password or a Testing-mode app; the Testing context is added when the build says so), Microsoft says consent is required, the session ended, the grant went unused for a while, a sign-in-frequency or other security policy asked for a new sign-in, or the account is blocked; the provider's own words stay under "Show details". The account is marked "Reconnect needed" with one notification; ReMa never opens a sign-in on its own. **Disconnect** stops syncing; the account's last connector also signs out: Google access is revoked at Google, Microsoft tokens are deleted (and the details link to your Microsoft account's app permissions). Your tracked applications and their history are kept.

**Sync** (only when Job Mail & Interview Sync runs). Gmail: the first run records the mailbox `historyId` and reads the lookback's job mail with a narrow dated search; later runs read `users.history.list` changes only; an expired history id leads to a bounded resync. Outlook: Microsoft Graph delta queries on the Inbox, bounded by the lookback, with the stored `@odata.deltaLink`; an invalid delta token leads to a bounded resync. A longer lookback reads the added days once (a dated search, a `receivedDateTime` range). At most one sync per connector runs at a time. One run reads at most 200 Gmail messages for a first sync or a longer lookback, and at most 2,000 Outlook changes; a mailbox with more is read from its newest mail, the run says so, and the next runs continue (Outlook from the page where it stopped, older mail by date) until the lookback is covered.

**From email to tracker.**

1. A deterministic prefilter scores each new message from its headers: a known application thread, recruiter or company domain, or an applicant-tracking system goes straight on; job words make a candidate; everything else is filtered, never read in full, and only its id, thread, date and sender domain are kept.
2. Candidates get a headers-only relevance check by your model.
3. Relevant messages are read in full (text only, truncated) and classified into one of 13 categories with a confidence, as strict JSON validated in Rust. Email text is delimited untrusted data; these requests carry no tools and no web access.
4. The tracker applies the result with an audit entry, oldest email first across both mailboxes. Only the newest email decides the status; confidence below 50% changes nothing and lists the email under "Needs review" (interview confirmations, reschedules, cancellations, rejections and offers need 80%). The same email in Gmail and Outlook is processed once.

**Interviews and your calendar.** Only confirmed interviews (never proposed times) whose date and time the email itself states are added. ReMa first checks every connected calendar, Google and Outlook, for overlapping events. If the time is free, it creates "Interview — Company — Role" with the company, role, time, interview type, interviewers, location and meeting link. If not, it flags the conflict in Applications and sends a notification naming the conflicting events, and creates nothing. Every event carries a private `remaInterviewId` property, so an event is never created twice; a reschedule updates the same event (a new time that conflicts is reported, not moved); a cancellation keeps the event, renamed and marked free; events you delete are not recreated.

**Background.** Mail is read only by Job Mail & Interview Sync, on its schedule, while ReMa runs. Two options are off until you turn them on: **Run ReMa in background** (closing the window keeps ReMa in the system tray, whose menu can also run Job Mail & Interview Sync) and **Start ReMa at login** (starts in the tray). When ReMa is not running, nothing syncs; no system service is installed. A second launch shows the running ReMa.

**In chat**, like Claude's and ChatGPT's connectors: every connected connector is on in a chat, and the composer's **+** menu has a **Connectors** group (Gmail, Google Calendar, Outlook Mail, Outlook Calendar, Applications) to turn one off for that chat; the choice stays with the chat. The model decides when to use them: it gets ReMa's connector tools (`mail_search`, `mail_get_message`, `mail_get_thread`, `calendar_list_events`, `calendar_check_availability`, `calendar_create_event`, `calendar_update_event`, `applications_find_match`, `applications_update_status`, `applications_append_timeline_event`) next to its web search. Mail tools return job-related mail only, marked as untrusted private data. Changes wait for your approval every time.

Web access and private data (Anthropic's guidance on web tools next to untrusted private data): the web stays available until a tool returns private data. From then on, OpenAI's and Anthropic's own search is taken out of the rest of that answer (the next round is sent without it), ReMa's web tools refuse, and every MCP call needs approval. Codex (ChatGPT account) and Gemini keep their search for a whole answer, so there an answer gets either the connectors (a question about your own mail, calendar or applications) or the model's search, never both. Once a chat has read private data, its later answers have no web access (the chat says so); job and company searches ReMa runs in it use only the words of the new message. A new chat has the web again.

**Professional networks.** LinkedIn sign-in (OpenID Connect with PKCE) identifies you; LinkedIn shares a member's connection list only with apps it approved for its Connections API, and XING offers no API for desktop apps. So **Your contacts** (under Professional networks) imports what both leave to you: your LinkedIn data export (the ZIP LinkedIn sends, or its `Connections.csv`; a newer export replaces the earlier one) and vCard files (contacts saved from XING, or an address book). Only name, company, position, profile link and connection date are kept, on this computer; never e-mail addresses or phone numbers. Network Connect checks them like LinkedIn's own list ("Your LinkedIn data export lists them at …"), ReMa matches them itself, and they are never sent to a model, copied out or used in Business.

Details, compliance and validation: [docs/connectors/](docs/connectors/implementation.md).

## Applications

One row per application, always in exactly one section:

| Section | What it holds | Last column |
|---|---|---|
| Interviews Confirmed | an interview with an agreed date and time | the interview ("Interview confirmed for Sep 29 at 10:00 CEST.") and a meeting link |
| Applications Confirmed | the employer confirmed the application | "Application received successfully. No action required." |
| Needs Your Action | a request (proposed interview times, an assessment, documents) or an offer | the requested action, from the email |
| Applications In Progress | under review, recruiter messages | the latest update, from the email |
| Rejected | an explicit rejection | the reason the email gives, or "No reason provided." |

Rows are sorted by the latest email; each section collapses and scrolls on its own. History is replayed in order, so an older email read later never moves an application back. An email is matched by thread, job ID, then company and role (the same company with another role, or the same role with another job ID, is another application); a new application after a rejection, or after 180 days of silence, starts a new row; an unclear match changes nothing and is listed for review. Chat's application tools read the same rows. Details: [docs/applications/implementation.md](docs/applications/implementation.md).

## Job Mail & Interview Sync

A built-in scheduled task (Scheduled Tasks → Built-in) and the only thing that reads your mail: connecting Gmail or Outlook reads nothing, and while the task is off (or paused) nothing is read, not even with "Run now". Setting it up needs a connected mailbox and shows the mail sources, **Initial lookback: last [N] days** (default 30), whether to add confirmed interviews to your calendar, optional instructions, the model, and the usual schedule (default every hour).

- The first run reads the last N days; later runs read only new mail.
- A longer lookback reads the added days once; a shorter one reads nothing old and deletes nothing.
- Progress (messages, cursor, how far back the mailbox was read) is saved in one transaction, only after a successful run.

## Calendar

The calendar button at the top right opens a week of your connected calendars inside ReMa (previous week, today, next week). Events are read live from Google Calendar and Outlook Calendar and never stored; ReMa's interviews are highlighted with company and role, link to their application and have a **Join** button for the meeting link. An interview that is in no calendar (a conflict, or no calendar connected) is shown with the reason; the same interview in two calendars is listed once. Without a calendar the panel says "No calendar connected. Connect Google Calendar or Outlook Calendar in Settings → Connectors."

## Profile

The Profile page has two independent parts, chosen with the tabs under its title. The selected part is part of the navigation state: uploading, previewing or re-rendering never switches it.

**Documents & Credentials** (default).

- **CVs.** Upload one or several files at once (PDF, Word `.docx`, text, Markdown). Each appears as a card with its type, size and date, and can be previewed, renamed, replaced, removed or marked as the **primary** CV. Files that cannot be added are listed with the reason. Nothing is copied into the Custom Profile.
- **Credentials.** Optional entries for degrees, professional and course certificates, training, licenses, badges and other evidence, each with an optional file (PDF, PNG, JPEG, WebP, Word) and optional details: title, issuer, issue and expiry dates, credential ID, URL and a note.
- **Preview.** Documents open inside ReMa: PDFs are drawn by PDF.js, images as images, and Word/text files as plain text blocks extracted by Rust (no HTML, scripts or macros ever reach the webview).
- **Storage rules.** The format is detected from the file's content (signatures), not only its extension; symbolic links and non-regular files are refused; files are limited to 20 MB; stored names are generated.

**Custom Profile** (optional). The structured form: personal details, professional title and summary, experience, education, skills, languages, links and custom fields. Empty fields are fine. **Fill from a CV…** is the only way to copy details from a document: Rust extracts the text, finds email addresses, phone numbers and links deterministically, and the default model structures the rest as JSON, which Rust validates (a name, email, phone or link that does not appear in the document is dropped). A review dialog shows every value; nothing is saved until you choose.

## Portfolio Studio

A page of its own, directly below Profile: CVs built in ReMa, separate from uploaded files (which are never changed).


- **Templates.** 12 designs in a registry (`src/lib/portfolio/templates.ts`): Minimal, Modern, Executive, Technical, Data / AI, Consulting, Product, Creative, Academic, Compact, Two-column and ATS-friendly. A template is data read by one layout engine (`src/lib/portfolio/layout.ts`), so adding one means adding an entry.
- **Editor.** Header and sections (summary, experience, projects, education, skills, languages, certifications, links, custom): add, remove, reorder, show or hide, rename. Switch templates, paper size (A4, Letter) and accent color at any time without losing content. Changes save automatically.
- **Preview and export.** pdfmake renders the PDF in the interface with embedded open-license fonts (Inter, Source Sans 3, Source Serif 4, Source Code Pro, Playfair Display); the live preview shows that same file through PDF.js, and **Export PDF** saves it where you choose (the save dialog runs in Rust). Text is real and selectable, margins follow the template, and long entries flow onto further pages without being cut.
- **Start.** A new CV starts with empty sections or a one-time copy of the Custom Profile and credentials.

## Built-in browser

The browser is a second Tauri webview (label `browser`) inside the main window. It is not a page in the navigation: it opens beside the current page when you click a link, or from the globe button in the header.

**Isolation.**

- ReMa's commands are ACL-checked through an app manifest (`src-tauri/build.rs` + `src-tauri/src/ipc_commands.rs`). They are granted only to the `main` webview on ReMa's own origin (`capabilities/default.json`). Remote pages get no capability, so every command and event listener is refused. This includes `get_profile`, the connector and chat commands, and the opener.
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

The **Profile** switch in the chat composer is the single control for career context. When it is on (it is saved per conversation), or when a scheduled prompt task has **Include my Profile** checked, `services/profile_context/` builds one normalized context from the Custom Profile, the primary CV, other CVs and credentials, for every message:

1. **Normalize.** CV text is split into sections, entries and facts; Custom Profile fields are mapped to the same fields (identity, summary, target roles, experience, skills, technologies, education, certifications, languages, locations, work and salary preferences, industries, projects, achievements, portfolio information, other context).
2. **Deduplicate.** The same skill, job, degree or language appears once.
3. **Precedence.** Custom Profile, then the primary CV, then other CVs, then credentials. For preferences (summary, target roles, locations, work and salary preferences) the highest source wins and the others are left out; everything else is merged. Credentials are objective records: they replace a vaguer CV mention and are added next to a Custom Profile fact, never overwriting it.
4. **Detail on demand.** A document's own text is added only when the message needs it: the whole CV when you ask to work on it or on a cover letter, otherwise up to three matching passages.

Every fact keeps its source, listed with the precedence at the end. Email addresses, phone numbers and personal details are removed; other files and Portfolio Studio CVs are named, never included. The fields are limited to about 14,000 characters. Extracted text is cached in SQLite when a document is stored, so nothing is re-read or uploaded in the background. Nothing is sent until you send a message; an edit to the Profile applies to the next message; with the switch off no Profile data is sent.

## Agents

An agent is a named, locally stored set of instructions. The **Agents** page (below Chat) lists the built-in agents and your own:

- Built-in agents are fixed; **Customize a copy** creates an editable custom agent.
- Custom agents can be created, edited, renamed, duplicated and deleted. Deleting one removes it from every chat that used it.
- **Use in chat** opens a new chat with the agent selected.

In Chat, the **+** menu selects any number of agents (up to 8). Their instructions are added to the system prompt once each, in the order you selected them. Selecting an agent never turns the Profile switch on. Selections are saved per conversation.

## MCP (Model Context Protocol)

**Settings → MCP** manages MCP servers. The client (`src-tauri/src/mcp/`) uses the official Rust SDK (`rmcp`) and speaks the current protocol (2026-07-28, stateless `server/discover`) with a fallback for servers on 2025-11-25.

- **Transports.** *Local program* (stdio): a program and its arguments, run directly (never through a shell). Shells, relative paths and shell syntax are refused; arguments are passed one per line exactly as written; it runs in its own process group and stops with the connection. Like Claude Code, the program gets your whole environment: ReMa's own plus what your login shell sets up (read once per run, as VS Code does, so tokens, proxies and version managers from your shell profile work when ReMa was opened from the Finder, the Dock or at login; without a `SHELL` variable ReMa asks your account record which shell you use), then the variables you set, which always win; ReMa's own `REMA_*` settings are not passed on. The program itself is found where your shell finds it: the login shell's `PATH` first, so nvm's `node` wins over a Homebrew or system copy, as in a terminal. Starting may take 90 s (a first `npx -y` or `uvx` run downloads the server) and a tool call 10 minutes; **Stop** ends an answer at once. *Remote server* (Streamable HTTP): an HTTPS URL (plain HTTP only for this computer) with no authentication, a bearer token, an API-key header, or **OAuth** sign-in in the browser (MCP authorization with a loopback redirect). A server still on the older HTTP+SSE transport (protocol 2024-11-05) connects too: when Streamable HTTP is refused, ReMa opens the URL's event stream as the MCP specification's backwards-compatibility section describes, and sends messages only to the address the server names on its own site. WebSocket is not an MCP transport and is not supported.
- **Secrets** (tokens, header values, environment values, OAuth credentials) are stored in the OS credential store and never shown again or sent to the interface; leaving a field empty keeps the stored value.
- **States.** A new server is **off**. *Configured* → *turned on* in Settings → *available* in the chat's + menu → *selected* in a chat → a tool *called* by the model. Only servers that are on appear in the + menu, and only the ones selected in a chat are offered to the model in that chat.
- **Approvals.** Tools the server marks read-only run directly; every other call shows its arguments and waits for **Allow once**, **Allow for this chat** or **Deny**. Stopping the answer denies what is still waiting.
- **Turning a server off** closes its connection (and stops a local program). Chats that selected it drop it with a short note; turning it on again does not re-add it.
- **Test**, **Connect/Reconnect**, **Disconnect**, **Sign in/Sign out**, **Edit** and **Remove** are on each server row; connection failures show the server's reason.
- **Import from other apps** adds the servers you already use in Claude Desktop, Claude Code (user and project servers in `~/.claude.json`), Cursor, VS Code or Windsurf, or from pasted JSON (an `mcpServers` block, VS Code's `servers`, or one server, as `claude mcp add-json` takes). `${VAR}`, `${VAR:-default}` and `${env:VAR}` are filled in from your environment when the server is added, so it also works when ReMa is not started from a terminal; values go to the credential store. `cmd /c npx …` becomes `npx …`. Servers on the older HTTP+SSE transport are added like any remote server. A server that starts through a shell, uses WebSocket, sends several headers or asks for VS Code inputs is listed with the reason instead of being added. Imported servers are on unless they were off in the other app.

## ReMa MCP (built in)

**ReMa MCP** is ReMa's own job-search MCP server, compiled into the app (`src-tauri/src/rema_mcp/`). It is **on by default**: while on, its tools are offered in every chat automatically, with no installation, configuration or per-chat selection. It does nothing until a model calls one of its tools for your request; there is no background crawling. Details, source rules and test results: [docs/rema-mcp](docs/rema-mcp/implementation.md).

| Tool | What it does |
| --- | --- |
| `search_jobs` | Finds vacancies with strict filters (title, skills, places, work mode, seniority, contract, working time, salary, posting age, language, sources) and returns compact, source-linked summaries with coverage, warnings and a cursor. |
| `get_job` / `get_jobs` | Reads one or up to 10 jobs in detail: description sections, requirements vs. nice-to-haves, salary as stated, posting/update/check dates, availability and evidence. |
| `search_similar_jobs` | Finds vacancies similar to a known job, excluding it and its duplicates. |
| `source_status` | Reports which sources are usable now, how (API, feed, page, links only), and their last check. |

- **How it runs.** Each chat answer opens an in-process MCP session (official Rust SDK `rmcp`, protocol 2026-07-28) between ReMa's MCP client and the ReMa MCP server over an in-memory stream. No process, port or file is involved.
- **Discovery.** ReMa's own no-key sources always search (employers' boards and public job boards, see [Career search](#career-search)); an optional search service (Settings → Career Search → Advanced) and the chat model's hosted web search add results, the latter through a narrow request with only the search terms and no tools, so ReMa MCP cannot call itself. `search_jobs` fails only when no route answered.
- **Sources.**
  - The documented public APIs and feeds of Greenhouse, Lever, Ashby, Personio, SmartRecruiters, Workable and Recruitee, and the public APIs of Arbeitnow, The Muse, Remotive and Hacker News ("Who is hiring?").
  - Employer career pages: one read of a public page, `JobPosting` markup preferred, `robots.txt` honored.
  - LinkedIn, XING and regional job boards are **discovery-only**. Their links are shown and labeled, but never fetched; ReMa MCP looks for the employer's own posting instead.
  - No logged-in scraping, cookies, CAPTCHA workarounds or private APIs.
- **Facts.** Unknown values stay unknown. Strict filters treat an unknown required value as no match and list it separately. Salaries are compared only in the same currency and period, and only as stated lower bounds. An update or search-index date is never a posting date. HTTP 200 alone never makes a job "active".
- **Safety.**
  - Public addresses only, enforced at connection time (no local network, metadata or DNS rebinding).
  - Every redirect is checked; sizes and times are bounded; no credentials leave the app.
  - Page text is data: instructions in a job ad are never followed, and links in descriptions are never fetched.
  - ReMa MCP never reads your Profile.
- **Cache.** Normalized jobs and 10-minute search snapshots are kept in ReMa's database under stable ids (`rj_…`). Raw pages are not kept. **Clear cache** removes only these.
- **Turning it off.** **Settings → MCP → Built-in** has a toggle. Turning it off takes two confirmations:
  1. "Disable ReMa MCP? Its built-in job-search and job-description tools will become unavailable." Buttons: Cancel / Continue.
  2. "Confirm disabling ReMa MCP? You can enable it again anytime in Settings." Buttons: Keep enabled / Disable ReMa MCP.

  Closing either dialog or pressing Escape cancels. Once off, its tools leave every chat, and new, queued and running calls are stopped in the backend. Saved jobs and chats are kept, and the choice survives restarts and updates. Turning it on again takes one click. It cannot be edited or removed.
- **Next to the model's own search.** When the chat model searches the web itself (see [How chats search](#how-chats-search)), ReMa MCP searches ReMa's sources and the optional service only, not the same provider a second time.
- **In other apps.** `ReMa mcp` serves ReMa MCP over stdio to Claude Desktop, Claude Code, Codex, Cursor, VS Code or any MCP client: no window opens, it uses the app's data folder (job cache, settings), and turning ReMa MCP off in Settings stops it there too. **Settings → MCP → ReMa MCP → Use in other apps** adds it to Claude Desktop, Cursor or VS Code in one click (their file is backed up first) and shows ready-to-copy setups for Claude Code (`claude mcp add --scope user rema -- <ReMa> mcp`) and Codex (`[mcp_servers.rema]` in `~/.codex/config.toml`). Its tool schemas use one type per value (nullable outputs as `anyOf`) and its errors come back as text with `isError`, so clients that check results against the output schema (the official TypeScript SDK does) and clients with a single-type schema dialect accept them.

## Bundled runtimes

Release builds include the official **Codex** runtime (latest `@openai/codex` from npm) and Anthropic's **`ant`** CLI (from Anthropic's Homebrew tap), so the account sign-ins work without installing anything. `pnpm build:app` downloads the current versions for the target platform (checking their published SHA-512 / SHA-256), then builds the app with them as sidecars (`src-tauri/tauri.runtimes.conf.json`). At run time ReMa uses the newest of the bundled and any installed copy. `pnpm tauri dev` and `pnpm tauri build` work without them.

## Career search

### How chats search

As in ChatGPT and Claude, a model whose provider hosts a web search (OpenAI with an API key or a ChatGPT account, Anthropic, Gemini 3) searches the web itself and writes the answer, for job, company, people and market questions too, with ReMa MCP's job tools next to its search. Its search is not kept to career sites; it is localized to the place the question names. Job tables from such answers reach Analytics only when the answer actually searched (a list from a model's memory is not stored). Models without a search of their own (local and compatible servers, Gemini before 3, a provider that refused its search) and chats that read private data get ReMa's search-first answers below.

**Settings → Career Search → How chats search the web**: *The model's own web search* (default) or *ReMa verified search*, which uses the search-first answers below for every model: ReMa searches, opens and checks each posting, and the model writes about what was found. Scheduled tasks follow the same choice.

### ReMa's search

Career search is built in: current jobs, companies, people and market facts
are searched whatever model is selected, with no search service or API key
to set up. Searches and page reads run in the background: ReMa never opens
a browser window, a terminal or a computer-use session to search. Details:
[docs/career-search/implementation.md](docs/career-search/implementation.md).

Every job search runs two routes at the same time (`src-tauri/src/career_search/`):

- **ReMa Jobs**: the ReMa MCP job engine over sources that need no key:
  employers' own boards (Greenhouse, Lever, Ashby, Personio, SmartRecruiters,
  Workable, Recruitee: found through the company's website, or learned from
  earlier results near the place) and public job boards (Arbeitnow, The Muse,
  Remotive, Hacker News "Who is hiring?").
- **The model's own search**, when it has one:

| Connection | Search request |
| --- | --- |
| OpenAI · API key | Responses API `web_search` with `external_web_access: true`, `filters.allowed_domains` (the career sites of the request's scope and region, at most 20), `user_location`, `tool_choice: "required"` for the search step, sources included; `url_citation`s read. |
| OpenAI · ChatGPT account | Codex web search, turned on per thread (`config: {"web_search": "live"}`, or the best mode the account allows: `modelProvider/capabilities/read`, `configRequirements/read`); the career sites go into its brief. |
| Anthropic · Claude Console or API key | Server tools `web_search` and `web_fetch` (`_20260209` on Claude 4.6+ and 5-series models, `web_search_20250305` before), with `allowed_domains` and `user_location`, up to 8 uses; `pause_turn` resumed; in-band errors reported; citations kept. |
| Gemini · API key | Grounding with Google Search. Next to function tools (MCP, ReMa's own tools), Gemini 3 models get Google Search with `toolConfig.includeServerSideToolInvocations: true` and their search calls go back verbatim in later rounds; Gemini 2.x cannot combine the two, so it gets ReMa's `rema_career_search` and `rema_read_page` tools instead. |
| Unsloth Studio | Its own `web_search` tool, and only that one: `enable_tools: true`, `enabled_tools: ["web_search"]`, `permission_mode: "off"`, `X-Unsloth-Events: 1` (recognised by its model list; leaving `enabled_tools` out would also enable its code-execution tools). |
| Other local and compatible endpoints (Ollama, LM Studio, vLLM…) | None: ReMa Jobs answers, and in normal chat the model gets ReMa's `rema_career_search` and `rema_read_page` tools. |

A model search counts only when the provider reports searches that actually
ran, and only pages it reported are used; if it fails or is unavailable, the
answer says so in one line and uses the other route ("Anthropic web search:
the search failed (…). Used ReMa Jobs instead.").

**ReMa's job searches search first.** With ReMa verified search, or for a model without a search of its own, when a message asks for current jobs
("Find me most recent AI jobs in Vienna Austria with salary starting from
85k a year.") or asks to check linked postings, ReMa itself (not the model)
runs this sequence, in chat and in scheduled tasks:

1. **Understand the request.** Rust reads the role and its close variants
   (AI → AI, ML, Applied AI and LLM Engineer…), the place (city, country,
   time zone), remote, the posting window, a salary floor and named companies.
2. **Search.** Both routes at once, each source isolated: one failing source
   never stops the others, and a source that keeps failing rests for a while.
3. **Validate.** Postings the model's search found are read from their own
   page (at most 16, 4 at a time, public addresses only, `JobPosting` data
   preferred). Postings that are gone, closed or expired, older than the
   window, elsewhere or off topic are dropped. A stated salary below the
   floor is left out; a posting that states none is shown with a note.
   Duplicates across sources are merged, keeping the employer's own posting
   ("also listed on Arbeitnow"); newest first.
4. **Answer.** ReMa writes the table: role, company, location, work mode,
   salary, posting date, a direct link named after its source, and a status
   with what else to know ("Verified posting · whether it is still open is
   not stated"). Below it: the salary summary, what was left out, sources
   that could not be searched and routes that failed. Then the model adds
   its assessment of those listings only, with web access and tools off; the
   listings reach it as delimited data it must not take instructions from.

"No matching postings" is an answer. When no route could search, the answer
says so and why ("ReMa could not retrieve live career sources for this
request right now. … What happened: …"), shows no listings and never asks
for a search service. Stop cancels searches and page reads. A search
repeated within ten minutes reuses the stored result and shows when its
sources were read.

**Research requests** about current companies, people and markets ("Who are
the current recruiters at Nordlicht AI?") search Wikidata, Wikipedia, the
company's own homepage, team and careers pages and its job boards, together
with the model's own search. The model answers only from what was found,
given as delimited data; emails and phone numbers are removed first; the
answer ends with a numbered **Sources** list. Questions about your own mail
and applications are not sent to the web.

- **What you see.** While it runs, the current step ("Searching jobs…",
  "Checking 3 postings…"), then **Searched career sources · engine · N
  searches · N pages read · N postings**; expand it for each query and each
  page checked.
- **Scheduled tasks** use the same searches; their runs record the stages,
  the search scope and the sources consulted.
- **Settings → Career Search — Automatic** shows the selected model's
  routes, **Check now** (one request per route), **Recent searches** and,
  under **Advanced (optional)**, a search service you may add (Brave,
  Tavily, SearXNG). It only adds results: career search works the same
  without it, and a misconfigured one is skipped. Keys go to the system
  keychain.
- ReMa's own extraction requests (reading a job list, CV import, mail
  triage and classification) never search the web.
- In debug builds, `REMA_DEV_ATS_BASE` points every no-key source at one
  local test server, `REMA_DEV_ALLOW_LOCAL_PAGES=1` lets ReMa read pages
  from it, and `REMA_ANTHROPIC_BASE_URL` / `REMA_OPENAI_BASE_URL` /
  `REMA_BRAVE_URL` / `REMA_TAVILY_URL` point providers and services at a
  mock (`scripts/e2e/mock-providers.mjs`).

## Analytics

**Opening it.** The **Analytics** button in the header opens a panel at half the width of the workspace, beside the current page. It is not a page in the navigation. The panel can be resized, collapsed to a strip, maximized, minimized to the header button, and closed. It reopens as you left it: size and mode are kept in the window, and the scope, filters, ranking, columns, tab and options are saved in SQLite. **Analyze** on a chat answer or task result opens it with that search selected. A job row opens the job in the built-in browser, and Analytics and the browser can sit side by side.

**Where the jobs come from.** Every chat answer and prompt-task result that contains a job table is read automatically when it finishes, and older history is read once at start. (The chat system prompt asks the model to list jobs as a table with company, role, location, work mode, salary, posting date, key skills and link.) **Analyze N jobs** under such an answer opens those jobs in Analytics (ranking, skill gaps, requirements, learning). **Analyze jobs** appears under answers that seem to link to postings without a table: it asks the model that wrote the answer (or the default model if that one is gone) to copy the list out, and keeps only values that appear verbatim in the answer. Answers that only suggest searches (links to search result pages) don't get the button; if nothing is found, ReMa says the answer lists no postings and how to ask for them. Each answer or run becomes a *job search run*, which keeps its own list of jobs even when those jobs are shared with other runs.

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
- A run is claimed and the schedule advanced *before* it starts, so it can never run twice. A task that is still running skips its next occurrence (recorded in its history as Skipped).
- Every execution is a run record created before the task starts (queued → running → succeeded, failed or cancelled), with a snapshot of the task, its stages, the context it used (names only, never credentials), its outputs by reference and a safe error. Runs left open when ReMa closed are marked "Execution interrupted before completion." at the next start. See [docs/run-history/implementation.md](docs/run-history/implementation.md).
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
| `pnpm brand`             | Regenerate the app icons and logo files from the master icon |

## Project structure

```text
rema/
├── src/                          # Frontend (React + TypeScript)
│   ├── app/                      # App root, navigation, sidebar entries
│   ├── pages/                    # ChatPage, AgentsPage, ScheduledTasksPage, ProfilePage, PortfolioStudioPage,
│   │                             # ApplicationsPage, SettingsPage
│   ├── assets/brand/             # Master app icon (rema-icon-source.png), in-app icon, logo lockups
│   ├── components/
│   │   ├── layout/               # AppShell, Sidebar, PageContainer, ThemeToggle, LaunchIntro, NotificationBell,
│   │   │                         # CalendarPanel
│   │   ├── browser/              # BrowserProvider, BrowserPanel, Auto Fill button and report
│   │   ├── analytics/            # AnalyticsPanel, scope/filter/ranking editors, Jobs, Skill gap,
│   │   │                         # Requirements and Learning tabs, charts, Analyze button
│   │   ├── chat/                 # Composer, + menu and chips, tool activity and approvals, MessageList, …
│   │   ├── agents/               # Agent icons, create/edit dialog
│   │   ├── profile/              # Profile tabs: documents & credentials (cards, viewer), Custom Profile
│   │   ├── portfolio/            # Portfolio Studio: gallery, new CV, editor, section editor
│   │   ├── pdf/                  # PDF.js page renderer (viewer and preview)
│   │   ├── tasks/                # TaskDialog, TaskDetail, JobReport, TaskActions, TaskStatus
│   │   ├── settings/             # Provider connections (account or key), endpoints, Career Search, MCP (built-in ReMa MCP + yours), Connectors
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
├── scripts/brand/                # Icon and logo generator (build-brand.mjs), outlined wordmark
├── scripts/runtimes/fetch.mjs    # Downloads the latest Codex and ant for bundling
│
└── src-tauri/src/                # Backend (Rust)
    ├── lib.rs                    # Startup: database, keychain, scheduler, IPC
    ├── ipc.rs                    # Command/event registration + bindings export
    ├── ipc_commands.rs           # Command list for the permission manifest (checked by a test)
    ├── commands/                 # Thin Tauri commands
    ├── services/                 # chat, providers, tasks, scheduler, schedule (pure math), system,
    │                             # profile, documents (storage + text extraction), profile_import, profile_context/,
    │                             # portfolio, agents, mcp, chat_tools (MCP tools in chat, approvals), websearch,
    │                             # connector_tools (mail/calendar/tracker tools in chat), mail_monitor (runs Job Mail &
    │                             # Interview Sync), calendar_view (in-app calendar), applications, notifications,
    │                             # background (tray, start at login)
    ├── mcp/                      # MCP client: config validation, connections (stdio/HTTP), OAuth
    ├── rema_mcp/                 # ReMa MCP: contracts, source registry, safe fetcher, ATS adapters,
    │                             # job engine, cache, in-process MCP server and host
    ├── protocol.rs               # rema-doc:// document files for the main window
    ├── browser/                  # Built-in browser: webview, navigation policy, Linux embedding, autofill
    ├── analytics/                # Job analytics: table/list parsing, normalize, skills dictionary, ingest
    │                             # (dedupe), page reader + background worker, dataset, filter, rank,
    │                             # matching, overview, gap, unique (requirements), learning
    ├── jobs/                     # Job-application pipeline: prefilter, extract (triage, classifier), applications,
    │                             # interviews, calendar_sync, tracker, report
    ├── accounts/                 # Provider account sign-in: Codex app-server client, Anthropic CLI,
    │                             # runtime discovery, sign-in sessions
    ├── connectors/               # Gmail, Google Calendar, Outlook Mail/Calendar: OAuth (PKCE), token manager,
    │                             # provider APIs, incremental sync, the old Google settings' migration
    ├── career_search/            # Career search: requirement and scopes, plan, source registry, router,
    │                             # no-key job sources, company research, health, status
    ├── retrieval/                # Job searches: intent, provider search, optional search services (Brave,
    │                             # Tavily, SearXNG), page validation, rendering, web tools for local models
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

The ReMa logo is the app icon: a glossy blue rounded tile with a white **R**, and a blue node in the R's bowl (the match). The same artwork is used everywhere; only its size changes.

The master is `src/assets/brand/rema-icon-source.png` (1024 × 1024, full-bleed tile, transparent outside the rounded corners). `pnpm brand` (`scripts/brand/build-brand.mjs`) generates everything else from it:

| Output | For | In the app |
| --- | --- | --- |
| `src-tauri/icons/` (`.icns`, `.ico`, PNGs) | macOS, Windows and Linux app icon: Dock, taskbar, launcher, bundles | Tauri bundle |
| `src/assets/brand/rema-icon.png` (256 px) | Every in-app use | `<BrandMark />`: title bar, launch intro, the new-chat screen; `<BrandLogo />` (icon + name) in Settings → About |
| `src/assets/brand/rema-logo-light.svg`, `rema-logo-dark.svg` | Icon + wordmark lockups for light and dark backgrounds (website, documents) | Not bundled |

- **Desktop icon grid**: the tile sits at 824 px on a 1024 px canvas with a soft shadow (the macOS icon grid), so it lines up with other apps in the Dock and the Windows taskbar.
- **Wordmark**: "ReMa" in Inter (weight 650, −2% tracking), outlined to paths (`scripts/brand/wordmark.json`) in the lockups; in the app it is live text next to the icon.
- **Changing the logo**: replace `rema-icon-source.png` and run `pnpm brand`. Resizing is done by `tauri icon`, so no image tools are needed.
