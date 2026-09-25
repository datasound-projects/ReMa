# ReMa

**Career UI Harness**: a fast, lightweight desktop app built on a deterministic Rust core with an embedded LLM layer.

Rust owns application state, persistence, scheduling, validation, provider configuration and execution. LLMs are used only to reason about prompts and write the responses.

## What ReMa does

- **Chat**: a general-purpose AI chat with streaming responses, Stop, Retry, Copy, Markdown (code, lists, tables, links) and persistent conversation history ("Recents").
- **Models**: OpenAI, Anthropic, Google Gemini, and any OpenAI-compatible endpoint (Ollama, LM Studio, vLLM, …). Pick the model in the chat composer.
- **Scheduled Tasks**: schedule the prompt you are writing, straight from the composer. Supports one-time, daily, every N days, selected weekdays, and intervals of 15 minutes or more. A task can end on a date or after N runs. You can edit, pause, resume, delete or run tasks now, and each task keeps its run history with results.
- **Settings**: connect providers, choose which models appear in the chat, and set the default model. Connect Google Workspace (Gmail and Calendar) with one sign-in.
- **Job applications (Gmail)**: a scheduled task type that finds job-application emails, keeps one record per application (Confirmed, Application in Process, Needs Your Action, Upcoming Interview, Rejected), and shows an overview table in the task history. It can add confirmed interviews to Google Calendar and report calendar conflicts.
- **Profile**: your career details, entered once. Import a CV (PDF, Word, text, Markdown) and review what ReMa read, or build it by hand. Also stores documents (CVs, certificates, portfolios) and custom fields.
- **Built-in browser**: links in Chat and task results open in a panel next to ReMa. You can collapse, maximize, dock left or right, and resize it. "Open in external browser" is always available.
- **ReMa Auto Fill**: fills the standard fields of an application form in the built-in browser from your Profile. You review and submit yourself. ReMa never submits.
- **Profile context**: a **Profile** switch in the chat composer (and an option on scheduled prompt tasks) gives the model a summary of your Profile. It is off unless you turn it on.

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
Rust services               src-tauri/src/services   (chat, providers, tasks, scheduler, schedule,
                                                      profile, documents, profile_import, profile_context)
                            src-tauri/src/jobs       (job-application workflow, run by the scheduler)
                            src-tauri/src/browser    (built-in browser webview, navigation policy, Auto Fill)
      ↓
Persistence / providers     src-tauri/src/db (SQLite) · src-tauri/src/llm (adapters) · src-tauri/src/secrets (OS keychain)
Integrations                src-tauri/src/integrations/google (OAuth, Gmail tool, Calendar tool)
```

Rules:

- **Deterministic core.** The scheduler decides when tasks run, using pure, unit-tested schedule math. The LLM never controls timing, state, credentials or configuration.
- **Provider-neutral.** Chat and scheduler use one `LanguageModel` interface. Provider request formats, authentication, streaming and errors stay inside `llm/openai.rs`, `llm/anthropic.rs` and `llm/gemini.rs`. The OpenAI adapter also serves every OpenAI-compatible endpoint.
- **Rust is the source of truth.** IPC types, commands and events are generated from Rust. The frontend never hand-writes copies of them.
- **Streaming via events.** Replies stream as typed `ChatEvent`s. Generation runs in the background, so switching chats does not interrupt it.

## Data and credentials

- **Database**: SQLite at `<app data dir>/rema.db`. On macOS that is `~/Library/Application Support/cloud.datasound.rema/`. Schema changes are versioned migrations in `src-tauri/src/db/migrations/`.
- **Profile documents**: copied into `<app data dir>/profile-documents/` under generated names. The original file name, size, SHA-256 and extracted text are stored in SQLite. The original file is never changed or moved.
- **Credentials**: API keys and Google OAuth tokens are stored in the operating system's credential store: macOS Keychain, Windows Credential Manager, or Secret Service on Linux. They never go in the database, config files or the frontend. The UI can only save, replace or remove a key.
- **Authentication**: API keys for OpenAI, Anthropic and Gemini. OpenAI-compatible endpoints take an optional key. None of the three cloud providers offers an official OAuth flow that a third-party desktop app can use for their model APIs without its own registered OAuth client. The credential model already supports OAuth tokens (with expiry), so an official flow can be added without changing chat or scheduler code.

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

The structured Profile in SQLite is the source of truth, and documents are attachments to it. Every field, list entry and document is editable, and list entries can be reordered.

**Supported documents.** PDF, Word (`.docx`), plain text and Markdown. Rust extracts their text. PNG and JPEG can be stored, but ReMa does not read them. The format is detected from the file's content, not only its extension. Files are limited to 20 MB. PDFs that are scanned images have no text to read.

**CV import.** Rust extracts the text, then finds email addresses, phone numbers and links deterministically. The default model structures the rest (experience, education, skills, languages) as JSON. Rust validates that JSON, and drops any name, email, phone or link that does not appear in the document. The result is only a proposal: a review dialog shows every value, pre-selects only empty fields, and never replaces an existing value unless you tick it. If no model is available, the deterministic details are still offered.

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

The context builder (`services/profile_context.rs`) creates a short, provider-independent summary from the structured Profile: name, title, location, summary, skills, experience, education, languages, links, custom fields, and document **names**. Email and phone are left out, and so is document text. It is added to the system prompt only when:

- the **Profile** switch in the chat composer is on for that conversation (it is saved per conversation), or
- a scheduled prompt task has **Include my Profile** checked.

Otherwise no Profile data is sent.

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
| `pnpm build`             | Typecheck and build the frontend only                     |
| `pnpm typecheck`         | Typecheck the frontend                                    |
| `pnpm lint`              | Lint the frontend with oxlint                             |
| `pnpm test:rust`         | Run the Rust tests (fails if bindings are stale)          |
| `pnpm bindings`          | Regenerate `src/generated/bindings.ts` from Rust          |

## Project structure

```text
rema/
├── src/                          # Frontend (React + TypeScript)
│   ├── app/                      # App root, navigation, sidebar entries
│   ├── pages/                    # ChatPage, ScheduledTasksPage, ProfilePage, SettingsPage
│   ├── components/
│   │   ├── layout/               # AppShell, Sidebar, PageContainer
│   │   ├── browser/              # BrowserProvider, BrowserPanel, Auto Fill button and report
│   │   ├── chat/                 # Composer, MessageList, Markdown, ModelSelector, ConversationList
│   │   ├── profile/              # Profile sections, documents, custom fields, import review
│   │   ├── tasks/                # TaskDialog, TaskDetail, JobReport, TaskActions, TaskStatus
│   │   ├── settings/             # Provider and endpoint rows, Google Workspace section
│   │   └── ui/                   # Menu, Dialog, IconButton, StatusIndicator, BrandMark
│   ├── hooks/                    # useAsyncData, useChat, useTasks, useProfile, useAutofill, …
│   ├── services/                 # ipc.ts (callBackend, ApiError), events.ts, one service per area
│   ├── generated/bindings.ts     # Generated from Rust (do not edit)
│   ├── lib/                      # markdown.ts (safe rendering), format.ts, taskForm.ts, profileMerge.ts
│   └── styles/                   # tokens → base → layout → components → chat/tasks/settings/profile/browser
│
└── src-tauri/src/                # Backend (Rust)
    ├── lib.rs                    # Startup: database, keychain, scheduler, IPC
    ├── ipc.rs                    # Command/event registration + bindings export
    ├── ipc_commands.rs           # Command list for the permission manifest (checked by a test)
    ├── commands/                 # Thin Tauri commands
    ├── services/                 # chat, providers, tasks, scheduler, schedule (pure math), system,
    │                             # profile, documents (storage + text extraction), profile_import, profile_context
    ├── browser/                  # Built-in browser: webview, navigation policy, Linux embedding, autofill
    ├── jobs/                     # Job-application workflow: extract, interviews, applications, calendar_sync, report
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

## Styling

All colors, type, spacing, radii and shadows are CSS custom properties in `src/styles/tokens.css`. The palette uses LinkedIn-style blue (`#0A66C2`), white, soft gray and dark text, with subtle borders and restrained shadows.
