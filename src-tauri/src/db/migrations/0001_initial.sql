-- ReMa initial schema.
-- Timestamps are Unix epoch milliseconds (UTC). Secrets are never stored here.

-- Configured LLM providers (credentials live in the OS credential store).
CREATE TABLE providers (
    id          TEXT PRIMARY KEY,   -- 'openai' | 'anthropic' | 'gemini' | 'custom-<n>'
    kind        TEXT NOT NULL CHECK (kind IN ('openai', 'anthropic', 'gemini', 'openai_compatible')),
    name        TEXT NOT NULL,
    base_url    TEXT,               -- only for OpenAI-compatible endpoints
    model       TEXT,               -- configured model of an OpenAI-compatible endpoint
    auth_method TEXT NOT NULL CHECK (auth_method IN ('none', 'api_key', 'oauth')),
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);

-- Models reported by a provider; `enabled` models appear in the chat selector.
CREATE TABLE provider_models (
    provider_id       TEXT NOT NULL REFERENCES providers (id) ON DELETE CASCADE,
    model_id          TEXT NOT NULL,
    display_name      TEXT NOT NULL,
    enabled           INTEGER NOT NULL DEFAULT 0 CHECK (enabled IN (0, 1)),
    max_output_tokens INTEGER,
    sort_order        INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (provider_id, model_id)
);

-- Small key/value preferences (e.g. the default chat model).
CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE conversations (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    title       TEXT NOT NULL,
    provider_id TEXT NOT NULL,      -- model last used; kept even if the provider is removed
    model_id    TEXT NOT NULL,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);

CREATE INDEX conversations_by_updated ON conversations (updated_at DESC);

CREATE TABLE messages (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    conversation_id INTEGER NOT NULL REFERENCES conversations (id) ON DELETE CASCADE,
    role            TEXT NOT NULL CHECK (role IN ('user', 'assistant')),
    content         TEXT NOT NULL,
    status          TEXT NOT NULL CHECK (status IN ('complete', 'streaming', 'stopped', 'error')),
    error           TEXT,
    provider_id     TEXT,           -- assistant messages: which model answered
    model_id        TEXT,
    created_at      INTEGER NOT NULL
);

CREATE INDEX messages_by_conversation ON messages (conversation_id, id);

CREATE TABLE scheduled_tasks (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT NOT NULL,
    prompt      TEXT NOT NULL,
    provider_id TEXT NOT NULL,
    model_id    TEXT NOT NULL,
    schedule    TEXT NOT NULL,      -- JSON-encoded `Schedule`
    timezone    TEXT NOT NULL,      -- IANA name, e.g. 'Europe/Vienna'
    start_at    INTEGER NOT NULL,
    end_at      INTEGER,            -- last instant a run may start
    max_runs    INTEGER CHECK (max_runs IS NULL OR max_runs > 0),
    run_count   INTEGER NOT NULL DEFAULT 0,   -- scheduled runs started so far
    enabled     INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    status      TEXT NOT NULL CHECK (status IN ('active', 'completed')),
    last_run_at INTEGER,
    next_run_at INTEGER,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);

CREATE INDEX scheduled_tasks_by_next_run ON scheduled_tasks (enabled, status, next_run_at);

CREATE TABLE task_executions (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id       INTEGER NOT NULL REFERENCES scheduled_tasks (id) ON DELETE CASCADE,
    trigger       TEXT NOT NULL CHECK (trigger IN ('scheduled', 'manual')),
    scheduled_for INTEGER,
    started_at    INTEGER NOT NULL,
    finished_at   INTEGER,
    status        TEXT NOT NULL CHECK (status IN ('running', 'succeeded', 'failed')),
    provider_id   TEXT NOT NULL,
    model_id      TEXT NOT NULL,
    prompt        TEXT NOT NULL,    -- snapshot of the prompt that ran
    result        TEXT,
    error         TEXT
);

CREATE INDEX task_executions_by_task ON task_executions (task_id, started_at DESC);
