-- A third MCP transport, 'sse': a remote server known to speak only the
-- older HTTP+SSE transport, opened without probing Streamable HTTP first.
-- SQLite cannot change a CHECK constraint in place, so the table is
-- rebuilt (foreign keys are off during migrations; the reference from
-- conversation_mcp_servers is checked before this commits).
CREATE TABLE mcp_servers_new (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT NOT NULL UNIQUE COLLATE NOCASE,
    transport   TEXT NOT NULL CHECK (transport IN ('stdio', 'http', 'sse')),
    command     TEXT NOT NULL DEFAULT '',
    args        TEXT NOT NULL DEFAULT '[]',
    env_names   TEXT NOT NULL DEFAULT '[]',
    cwd         TEXT NOT NULL DEFAULT '',
    url         TEXT NOT NULL DEFAULT '',
    auth        TEXT NOT NULL DEFAULT 'none' CHECK (auth IN ('none', 'bearer', 'header', 'oauth')),
    header_name TEXT NOT NULL DEFAULT '',
    enabled     INTEGER NOT NULL DEFAULT 0 CHECK (enabled IN (0, 1)),
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);
INSERT INTO mcp_servers_new (id, name, transport, command, args, env_names, cwd, url, auth,
                             header_name, enabled, created_at, updated_at)
    SELECT id, name, transport, command, args, env_names, cwd, url, auth,
           header_name, enabled, created_at, updated_at
    FROM mcp_servers;
DROP TABLE mcp_servers;
ALTER TABLE mcp_servers_new RENAME TO mcp_servers;
