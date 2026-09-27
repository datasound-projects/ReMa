-- Network Connect: LinkedIn and XING join the shared connectors (one
-- connector registry, one token store). Tokens still live only in the OS
-- credential store.

CREATE TABLE connector_accounts_new (
    provider       TEXT PRIMARY KEY
        CHECK (provider IN ('google', 'microsoft', 'linkedin', 'xing')),
    account_id     TEXT,
    email          TEXT,
    display_name   TEXT,
    granted_scopes TEXT NOT NULL DEFAULT '',
    status         TEXT NOT NULL CHECK (status IN ('connected', 'reauth_required')),
    status_reason  TEXT,
    connected_at   INTEGER NOT NULL,
    updated_at     INTEGER NOT NULL
);
INSERT INTO connector_accounts_new (provider, account_id, email, display_name, granted_scopes,
        status, status_reason, connected_at, updated_at)
    SELECT provider, account_id, email, display_name, granted_scopes, status, status_reason,
        connected_at, updated_at
    FROM connector_accounts;
DROP TABLE connector_accounts;
ALTER TABLE connector_accounts_new RENAME TO connector_accounts;

CREATE TABLE connectors_new (
    id                     TEXT PRIMARY KEY
        CHECK (id IN ('gmail', 'google_calendar', 'outlook_mail', 'outlook_calendar',
                      'linkedin', 'xing')),
    enabled                INTEGER NOT NULL DEFAULT 0 CHECK (enabled IN (0, 1)),
    background_sync        INTEGER NOT NULL DEFAULT 1 CHECK (background_sync IN (0, 1)),
    last_sync_started_at   INTEGER,
    last_sync_completed_at INTEGER,
    last_success_at        INTEGER,
    last_error             TEXT,
    last_error_detail      TEXT,
    next_sync_at           INTEGER,
    updated_at             INTEGER NOT NULL DEFAULT 0
);
INSERT INTO connectors_new (id, enabled, background_sync, last_sync_started_at,
        last_sync_completed_at, last_success_at, last_error, last_error_detail, next_sync_at,
        updated_at)
    SELECT id, enabled, background_sync, last_sync_started_at, last_sync_completed_at,
        last_success_at, last_error, last_error_detail, next_sync_at, updated_at
    FROM connectors;
DROP TABLE connectors;
ALTER TABLE connectors_new RENAME TO connectors;
-- Professional networks never sync in the background.
INSERT INTO connectors (id, background_sync) VALUES ('linkedin', 0), ('xing', 0);
