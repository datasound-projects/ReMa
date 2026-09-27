-- ReMa MCP: normalized job records (no raw pages), the keys that identify
-- them (source job ids, canonical URLs), and short-lived search snapshots.
-- Clearing these tables never touches chats, Analytics or Profile data.

CREATE TABLE rema_mcp_jobs (
    id              TEXT PRIMARY KEY,
    record          TEXT NOT NULL,
    schema_version  INTEGER NOT NULL,
    first_seen_at   INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL
);

CREATE TABLE rema_mcp_job_keys (
    key     TEXT PRIMARY KEY,
    job_id  TEXT NOT NULL REFERENCES rema_mcp_jobs (id) ON DELETE CASCADE
);
CREATE INDEX rema_mcp_job_keys_job ON rema_mcp_job_keys (job_id);

CREATE TABLE rema_mcp_snapshots (
    id          TEXT PRIMARY KEY,
    query_key   TEXT NOT NULL,
    created_at  INTEGER NOT NULL,
    expires_at  INTEGER NOT NULL,
    data        TEXT NOT NULL
);
CREATE INDEX rema_mcp_snapshots_query ON rema_mcp_snapshots (query_key, expires_at);
