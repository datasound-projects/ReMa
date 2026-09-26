-- Profile documents and credentials, Portfolio Studio, Agents, MCP servers,
-- and each conversation's selected Agents and MCP servers.
--
-- Migrations run with foreign keys switched off (checked before commit), so
-- rebuilding `profile_documents` keeps the rows that reference it.

-- ── Profile documents ────────────────────────────────────────────────
-- Rebuilt to accept WEBP images and to mark one CV as the primary one.
CREATE TABLE profile_documents_v2 (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    name          TEXT NOT NULL,
    kind          TEXT NOT NULL CHECK (kind IN ('cv', 'certificate', 'portfolio', 'other')),
    format        TEXT NOT NULL
                  CHECK (format IN ('pdf', 'docx', 'text', 'markdown', 'png', 'jpeg', 'webp')),
    original_name TEXT NOT NULL,
    file_name     TEXT NOT NULL UNIQUE,  -- generated: <random hex>.<ext>
    size          INTEGER NOT NULL,
    sha256        TEXT NOT NULL,
    text          TEXT,                  -- extracted text (NULL for images or unreadable files)
    is_primary    INTEGER NOT NULL DEFAULT 0 CHECK (is_primary IN (0, 1)),
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL
);

INSERT INTO profile_documents_v2
    (id, name, kind, format, original_name, file_name, size, sha256, text, created_at, updated_at)
SELECT id, name, kind, format, original_name, file_name, size, sha256, text, created_at, updated_at
FROM profile_documents;

DROP TABLE profile_documents;
ALTER TABLE profile_documents_v2 RENAME TO profile_documents;

-- At most one primary CV. The newest existing CV becomes the primary one.
CREATE UNIQUE INDEX profile_documents_primary ON profile_documents (is_primary) WHERE is_primary = 1;
UPDATE profile_documents SET is_primary = 1
WHERE id = (SELECT id FROM profile_documents WHERE kind = 'cv' ORDER BY created_at DESC, id DESC LIMIT 1);

-- ── Credentials ──────────────────────────────────────────────────────
-- Degrees, certificates, licenses, badges and other evidence. The file (if
-- any) is a profile document; every other field is optional metadata.
CREATE TABLE credentials (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    kind            TEXT NOT NULL CHECK (kind IN ('degree', 'professional_certificate',
                        'course_certificate', 'training', 'license', 'badge', 'other')),
    title           TEXT NOT NULL,
    issuer          TEXT NOT NULL DEFAULT '',
    issue_date      TEXT NOT NULL DEFAULT '',  -- YYYY, YYYY-MM or YYYY-MM-DD
    expiration_date TEXT NOT NULL DEFAULT '',
    credential_id   TEXT NOT NULL DEFAULT '',
    credential_url  TEXT NOT NULL DEFAULT '',
    note            TEXT NOT NULL DEFAULT '',
    document_id     INTEGER REFERENCES profile_documents (id) ON DELETE SET NULL,
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL
);

-- Certificates already in the document library become credentials.
INSERT INTO credentials (kind, title, document_id, created_at, updated_at)
SELECT 'other', name, id, created_at, updated_at FROM profile_documents WHERE kind = 'certificate';

-- ── Portfolio Studio ─────────────────────────────────────────────────
-- CVs built in ReMa. `content` is validated JSON (header and sections);
-- templates are code, identified by `template_id`.
CREATE TABLE portfolio_documents (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT NOT NULL,
    template_id TEXT NOT NULL,
    page_size   TEXT NOT NULL CHECK (page_size IN ('a4', 'letter')),
    accent      TEXT NOT NULL DEFAULT '',   -- '' = the template's own color
    content     TEXT NOT NULL,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);

-- ── Agents ───────────────────────────────────────────────────────────
-- The user's own Agents. Built-in Agents are part of ReMa, not stored.
CREATE TABLE custom_agents (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    name         TEXT NOT NULL,
    description  TEXT NOT NULL DEFAULT '',
    instructions TEXT NOT NULL,
    icon         TEXT NOT NULL DEFAULT 'spark',
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL
);

-- ── MCP servers ──────────────────────────────────────────────────────
-- Configuration only. Secrets (environment values, tokens, OAuth
-- credentials) live in the OS credential store.
CREATE TABLE mcp_servers (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT NOT NULL UNIQUE COLLATE NOCASE,
    transport   TEXT NOT NULL CHECK (transport IN ('stdio', 'http')),
    command     TEXT NOT NULL DEFAULT '',   -- stdio: the executable
    args        TEXT NOT NULL DEFAULT '[]', -- stdio: JSON array of arguments
    env_names   TEXT NOT NULL DEFAULT '[]', -- stdio: JSON array of variable names
    cwd         TEXT NOT NULL DEFAULT '',   -- stdio: working folder ('' = ReMa's)
    url         TEXT NOT NULL DEFAULT '',   -- http: the MCP endpoint
    auth        TEXT NOT NULL DEFAULT 'none' CHECK (auth IN ('none', 'bearer', 'header', 'oauth')),
    header_name TEXT NOT NULL DEFAULT '',   -- http + 'header': e.g. X-API-Key
    enabled     INTEGER NOT NULL DEFAULT 0 CHECK (enabled IN (0, 1)),
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);

-- ── Chat selections ──────────────────────────────────────────────────
-- Agents chosen for a conversation, in the order the user added them.
-- `agent_id` is 'builtin:<slug>' or 'custom:<id>'.
CREATE TABLE conversation_agents (
    conversation_id INTEGER NOT NULL REFERENCES conversations (id) ON DELETE CASCADE,
    agent_id        TEXT NOT NULL,
    position        INTEGER NOT NULL,
    PRIMARY KEY (conversation_id, agent_id)
);

-- MCP servers made available to the model in a conversation.
CREATE TABLE conversation_mcp_servers (
    conversation_id INTEGER NOT NULL REFERENCES conversations (id) ON DELETE CASCADE,
    server_id       INTEGER NOT NULL REFERENCES mcp_servers (id) ON DELETE CASCADE,
    position        INTEGER NOT NULL,
    PRIMARY KEY (conversation_id, server_id)
);

-- Tool calls made while answering (JSON array), shown with the message.
ALTER TABLE messages ADD COLUMN activity TEXT;
