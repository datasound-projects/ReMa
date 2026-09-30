<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/media/logo-dark.png">
    <img src="docs/media/logo-light.png" alt="ReMa" width="280">
  </picture>
</p>

<h1 align="center">Your AI powered Career Desk</h1>

<p align="center">
  A desktop app that searches jobs, tracks your applications and interviews from your mailbox,<br>
  keeps your CV in shape and talks to the AI model <em>you</em> choose. Everything stays on your computer.
</p>

<p align="center">
  <img alt="Status: in development" src="https://img.shields.io/badge/status-in%20development-orange">
  <img alt="Version 0.1.0" src="https://img.shields.io/badge/version-0.1.0-blue">
  <img alt="Platforms" src="https://img.shields.io/badge/platforms-macOS%20%7C%20Windows%20%7C%20Linux-lightgrey">
  <img alt="Built with Tauri 2, Rust and React" src="https://img.shields.io/badge/built%20with-Tauri%202%20%C2%B7%20Rust%20%C2%B7%20React-24C8DB">
</p>

<p align="center">
  <img src="docs/media/demo-chat.gif" alt="Asking ReMa for AI engineer jobs in Vienna: it searches the web and streams a table of verified openings" width="900">
</p>

> [!IMPORTANT]
> **ReMa is in active development.** This is an early version: features are still changing, there are no signed installers yet, and some connectors need ReMa's own app registrations before they work outside a development build. You are welcome to try it, build it from source and open issues, but do not expect a finished product.

---

## What is ReMa?

ReMa is a lightweight desktop app (Tauri 2) with a deterministic **Rust core** and an **AI layer** you control. Rust owns your data, schedules, credentials and every decision about *when* something happens; the model only reasons about your prompts and writes the answers.

- **Bring your own model.** OpenAI (with a ChatGPT account or an API key), Anthropic (Claude account or API key), Google Gemini, or any OpenAI-compatible endpoint such as Ollama, LM Studio or vLLM.
- **Your data stays local.** Chats, applications, profile and documents live in a SQLite database on your computer. There is no ReMa server. Your conversations, however, are processed by the AI model provider of your choice.
- **Credentials stay in the system keychain.** API keys and sign-in tokens go to macOS Keychain, Windows Credential Manager or Secret Service on Linux, never into the database, logs or a model's context.

## Key features (current version)

| | Feature | What it does today |
|---|---|---|
| 💬 | **Chat** | Streaming answers with Markdown, tables and links, persistent history, light and dark theme. Job questions always search real sources first and show verified postings with direct links. |
| 🔎 | **Career search built in** | Every job search runs ReMa's own no-key sources (employer boards and public job boards) next to the model's own web search, verifies each posting and returns a clean table. No search service or key to set up. |
| 📬 | **Job Mail & Interview Sync** | Connect Gmail or Outlook once. A built-in scheduled task reads only job-related mail, keeps your applications up to date and adds confirmed interviews to your calendar when the time is free. It is the only part of ReMa that reads mail. |
| 📋 | **Applications** | One row per application in five sections: Interviews Confirmed, Applications Confirmed, Needs Your Action, In Progress and Rejected, with a timeline that says where every change came from. |
| 📅 | **Calendar** | A week view of your connected Google and Outlook calendars inside ReMa, with interviews highlighted and their meeting links one click away. |
| 👤 | **Profile** | Your CVs and credentials with in-app preview, plus an optional structured profile. One switch in the chat composer gives the model your career context. |
| 🎨 | **Portfolio Studio** | CVs and cover letters from 20 + 5 templates, edited on an interactive canvas with an AI assistant, exported as PDF. |
| 🤖 | **Agents** | Six built-in agents (Job Search, Job Match Analyst, CV Tailoring, Interview Prep, Application Strategist, Career Research) and your own custom agents, selectable per chat. |
| ⏰ | **Scheduled Tasks** | Schedule any prompt straight from the composer: once, daily, weekdays, intervals. Every run keeps its result, progress and context. |
| 📊 | **Analytics** | A dashboard over every job ReMa has found: filter, rank, compare requirements with your Profile (skill gap) and research learning resources for the gaps that matter. |
| 🔌 | **MCP** | Connect local or remote MCP servers and pick them per chat. ReMa also ships its own job-search MCP server, which you can use from Claude Desktop, Claude Code, Cursor or VS Code. |
| 🌐 | **Built-in browser & Auto Fill** | Open postings in a panel next to ReMa and fill standard application forms from your Profile. You review and submit yourself. |
| 🧭 | **Network Connect & Business** | Early versions of two more workspaces: research the people behind companies and jobs, and find clients or contract work for what you offer. |
| 🔒 | **Privacy by design** | Every change a tool wants to make waits for your approval. Mail bodies are treated as untrusted data. A web search is never sent in the same request as your private data. |

## See it in action

**Connect an account once, stay connected.** Sign in with Google or Microsoft in your own browser; ReMa never sees your password.

<p align="center">
  <img src="docs/media/demo-connect.gif" alt="Connecting Google and Microsoft accounts in Settings" width="900">
</p>

<table>
  <tr>
    <td width="50%"><img src="docs/media/welcome.png" alt="Welcome screen"></td>
    <td width="50%"><img src="docs/media/chat-jobs.png" alt="A job search answer with a verified job table"></td>
  </tr>
  <tr>
    <td><b>Start a chat.</b> Pick a model, switch your Profile on, or schedule the prompt.</td>
    <td><b>Job search.</b> ReMa searched the web, verified the postings and shows them as a table with links.</td>
  </tr>
  <tr>
    <td><img src="docs/media/job-mail-sync-run.png" alt="A Job Mail & Interview Sync run"></td>
    <td><img src="docs/media/applications.png" alt="The Applications page"></td>
  </tr>
  <tr>
    <td><b>Job Mail & Interview Sync.</b> One run: 8 applications updated, 2 interviews found, every step recorded.</td>
    <td><b>Applications.</b> Interviews, confirmations, actions and rejections, straight from your mail.</td>
  </tr>
  <tr>
    <td><img src="docs/media/calendar.png" alt="The in-app calendar panel"></td>
    <td><img src="docs/media/settings-connectors.png" alt="Connected Google and Microsoft accounts"></td>
  </tr>
  <tr>
    <td><b>Calendar.</b> Your Google and Outlook week with interviews marked and a Join button.</td>
    <td><b>Connectors.</b> Google and Microsoft connected; each account shows exactly what it can do.</td>
  </tr>
  <tr>
    <td><img src="docs/media/account-manage.png" alt="Managing a connected Google account"></td>
    <td><img src="docs/media/agents.png" alt="The Agents page"></td>
  </tr>
  <tr>
    <td><b>Account details.</b> Permissions, health, last sync, and per-capability disconnect.</td>
    <td><b>Agents.</b> Built-in agents to start with, custom agents for your own instructions.</td>
  </tr>
  <tr>
    <td><img src="docs/media/cv-templates.png" alt="Portfolio Studio CV templates"></td>
    <td><img src="docs/media/cv-editor.png" alt="The Portfolio Studio editor"></td>
  </tr>
  <tr>
    <td><b>Portfolio Studio.</b> 20 CV templates and 5 cover letter templates with live previews.</td>
    <td><b>Editor.</b> Edit the document on the page, tune the design, ask the AI assistant, export a PDF.</td>
  </tr>
  <tr>
    <td><img src="docs/media/analytics.png" alt="The Analytics panel"></td>
    <td><img src="docs/media/scheduled-tasks.png" alt="Scheduled Tasks"></td>
  </tr>
  <tr>
    <td><b>Analytics.</b> Rank the jobs you found and compare their requirements with your Profile.</td>
    <td><b>Scheduled Tasks.</b> Built-in automation and your own prompts on a schedule.</td>
  </tr>
  <tr>
    <td><img src="docs/media/settings-mcp.png" alt="MCP settings"></td>
    <td><img src="docs/media/chat-dark.png" alt="Chat in dark mode"></td>
  </tr>
  <tr>
    <td><b>MCP.</b> ReMa's built-in job-search server plus any MCP server you add or import.</td>
    <td><b>Dark mode.</b> One click at the top right; ReMa remembers your choice.</td>
  </tr>
</table>

<p align="center">
  <img src="docs/media/profile.png" alt="The Profile page" width="49%">
  <img src="docs/media/business.png" alt="The Business page" width="49%">
</p>

## How it works

```text
React interface  →  typed IPC (generated from Rust)  →  Rust services  →  SQLite · OS keychain
                                                          ├─ chat, tools, scheduler, run history
                                                          ├─ career search, analytics, MCP client + ReMa MCP
                                                          ├─ connectors (OAuth + PKCE, Gmail, Outlook, calendars)
                                                          └─ model adapters (OpenAI, Anthropic, Gemini, OpenAI-compatible)
```

- The scheduler, the application tracker and every validation are deterministic Rust code. The model never controls timing, state or credentials.
- One model interface for every provider; provider, transport and sign-in are separate concepts.
- Replies and tool activity stream to the interface as typed events, so switching chats never interrupts a running answer.

## Getting started (from source)

Prerequisites: Rust 1.88+, Node.js 20.19+ or 22.12+, pnpm 10 and the [Tauri 2 platform dependencies](https://v2.tauri.app/start/prerequisites/). On Linux you also need a Secret Service provider such as GNOME Keyring.

```bash
pnpm install
pnpm tauri dev
```

Then open **Settings**, connect a model provider (or add a local endpoint such as `http://localhost:11434/v1` for Ollama) and start chatting.

Google and Microsoft connectors use ReMa's own app registrations. A build made without them shows **Set up** on the connector card: enter the registration once inside the app and the sign-in works right away. [docs/connectors/registration.md](docs/connectors/registration.md) explains how to create the registrations.

## Documentation

- [Handbook](docs/handbook.md): every feature in detail, architecture, data and credentials, scripts, building and signing.
- [Connectors](docs/connectors/implementation.md), [Career search](docs/career-search/implementation.md), [ReMa MCP](docs/rema-mcp/implementation.md), [Applications](docs/applications/implementation.md), [Portfolio Studio](docs/portfolio-studio.md), [Analytics and audit](docs/audit/implementation.md).
- [Known open issues](docs/parity/open-issues.md).

## What's next

- Signed and notarized installers for macOS, Windows and Linux.
- Published Google and Microsoft app registrations, so sign-in works in every build without setup.
- Rounding out Network Connect and Business.

Found a bug or have an idea? Open an issue. ReMa is built in the open, and feedback at this stage shapes what comes next.
