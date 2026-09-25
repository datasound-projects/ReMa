# ReMa

**Career UI Harness**: a fast, lightweight desktop app for career search, built on a deterministic Rust core.

ReMa is built one working feature at a time. This repository currently holds the **foundation**: the desktop shell, the Rust application core, the typed bridge between them, and the base UI.

## Architecture

```text
Frontend UI            React components and pages (src/pages, src/components)
    ↓
Frontend service layer typed calls to the backend (src/services)
    ↓
Tauri commands         thin IPC adapters (src-tauri/src/commands)
    ↓
Rust services          deterministic business logic (src-tauri/src/services)
    ↓
Data / external        storage, APIs, and later LLM tools (not yet added)
```

Principles:

- **Deterministic core.** Data, filters, state, workflows, ranking and UI behavior live in Rust services.
- **LLMs only where semantic reasoning helps.** They will call the core through explicit, structured tools and never control the whole app.
- **Thin edges.** React components never call `invoke` directly, and Tauri commands never contain business logic.
- **One typed contract.** Rust models serialize to JSON that matches the TypeScript types in `src/types`.

## Tech stack

Rust · Tauri 2 · React 19 · TypeScript · Vite · pnpm. There is no router, state library or CSS framework; they will be added only when a feature needs one.

## Prerequisites

- **Rust** (stable, 1.77.2+): <https://rustup.rs>
- **Node.js** 20.19+ or 22.12+
- **pnpm** 10: `corepack enable`
- **Platform dependencies for Tauri**: <https://v2.tauri.app/start/prerequisites/>
  - macOS: Xcode Command Line Tools (`xcode-select --install`)
  - Linux (Debian/Ubuntu): `sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev`

## Getting started

```bash
pnpm install
pnpm tauri dev      # launches the desktop app with hot reload
```

The first run compiles the Rust dependencies and takes a few minutes. Later runs start in seconds.

## Scripts

| Command                  | What it does                                              |
| ------------------------ | --------------------------------------------------------- |
| `pnpm tauri dev`         | Run the desktop app in development mode                   |
| `pnpm tauri build`       | Build an optimized, bundled release of the app            |
| `pnpm build`             | Typecheck and build the frontend only                     |
| `pnpm typecheck`         | Typecheck the frontend                                    |
| `pnpm test:rust`         | Run the Rust unit tests                                   |
| `pnpm dev`               | Frontend only, in a browser (backend shows "Unavailable") |

## Project structure

```text
rema/
├── src/                         # Frontend (React + TypeScript)
│   ├── main.tsx                 # Entry point
│   ├── app/                     # App root and page registry (pages.ts)
│   ├── components/
│   │   ├── layout/              # AppShell, Sidebar, PageContainer
│   │   ├── ui/                  # StatusIndicator, BrandMark
│   │   └── icons.tsx            # Inline SVG icons
│   ├── pages/                   # One component per sidebar page
│   ├── services/                # Backend access: ipc.ts (typed invoke) + feature services
│   ├── hooks/                   # React hooks that bind services to UI state
│   ├── types/                   # Shared TS types, including the command contract
│   ├── styles/                  # tokens.css → base.css → layout.css → components.css
│   └── assets/                  # rema-mark.svg (also the source of the app icons)
│
├── src-tauri/                   # Backend (Rust + Tauri)
│   ├── src/
│   │   ├── main.rs              # Desktop entry point; calls rema_lib::run()
│   │   ├── lib.rs               # App builder: state, command registration
│   │   ├── commands/            # Thin #[tauri::command] adapters
│   │   ├── services/            # Business logic (unit-tested)
│   │   ├── models/              # Serializable data types
│   │   ├── state.rs             # Shared AppState managed by Tauri
│   │   └── error.rs             # AppError → { code, message } for the UI
│   ├── capabilities/            # Tauri permission sets
│   ├── icons/                   # Generated app icons
│   ├── Cargo.toml
│   └── tauri.conf.json
│
├── index.html
├── package.json
└── vite.config.ts
```

The Rust core is a library (`rema_lib`) with a thin binary on top, so services can be unit-tested without a window and could later be reused by another front end, such as a CLI or an LLM tool server.

## Adding a feature

**A new backend command** (for example, `get_app_status`):

1. Add the data types to `src-tauri/src/models/`.
2. Add the logic to `src-tauri/src/services/`, with unit tests.
3. Add a thin `async` command to `src-tauri/src/commands/` that returns `AppResult<T>`.
4. Register the command in `tauri::generate_handler![...]` in `src-tauri/src/lib.rs`.
5. Mirror the types in `src/types/` and add the command to `CommandMap` in `src/types/api.ts`.
6. Expose it through a function in `src/services/`, and use it from a hook or page.

**A new page:** create it in `src/pages/`, then add one entry to `PAGES` in `src/app/pages.ts`. The sidebar picks it up automatically.

## Styling

All colors, type, spacing, radii and shadows are CSS custom properties in `src/styles/tokens.css`. The palette follows the ReMa website: LinkedIn-style blue (`#0A66C2`), white surfaces, soft-gray backgrounds and dark text, with subtle borders and restrained shadows. Components use these tokens and never hard-code values.
