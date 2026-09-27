-- Connectors: Gmail, Google Calendar, Outlook Mail and Outlook Calendar.
-- OAuth tokens are never stored here: they live in the OS credential store.

-- One connected account per provider (multi-account later: key by account_id).
CREATE TABLE connector_accounts (
    provider       TEXT PRIMARY KEY CHECK (provider IN ('google', 'microsoft')),
    account_id     TEXT,                -- the provider's stable user id
    email          TEXT,
    display_name   TEXT,
    granted_scopes TEXT NOT NULL DEFAULT '',   -- space-separated, as granted
    status         TEXT NOT NULL CHECK (status IN ('connected', 'reauth_required')),
    status_reason  TEXT,
    connected_at   INTEGER NOT NULL,
    updated_at     INTEGER NOT NULL
);

-- The user's choice per connector and its synchronization state.
CREATE TABLE connectors (
    id                     TEXT PRIMARY KEY
        CHECK (id IN ('gmail', 'google_calendar', 'outlook_mail', 'outlook_calendar')),
    enabled                INTEGER NOT NULL DEFAULT 0 CHECK (enabled IN (0, 1)),
    background_sync        INTEGER NOT NULL DEFAULT 1 CHECK (background_sync IN (0, 1)),
    last_sync_started_at   INTEGER,
    last_sync_completed_at INTEGER,
    last_success_at        INTEGER,
    last_error             TEXT,        -- user-readable
    last_error_detail      TEXT,        -- technical (never tokens)
    next_sync_at           INTEGER,
    updated_at             INTEGER NOT NULL DEFAULT 0
);

INSERT INTO connectors (id) VALUES ('gmail'), ('google_calendar'), ('outlook_mail'), ('outlook_calendar');

-- Incremental sync position per provider, account and resource
-- (Gmail historyId, Microsoft Graph deltaLink).
CREATE TABLE sync_cursors (
    provider   TEXT NOT NULL,
    account_id TEXT NOT NULL,
    resource   TEXT NOT NULL,
    cursor     TEXT NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (provider, account_id, resource)
);

-- Messages from any mail provider. Subject and sender are kept only for
-- job-related messages (shown as correspondence); never bodies.
CREATE TABLE mail_messages_new (
    provider        TEXT NOT NULL,
    message_id      TEXT NOT NULL,
    thread_id       TEXT NOT NULL,
    received_at     INTEGER NOT NULL,
    sender_domain   TEXT,
    sender          TEXT,
    subject         TEXT,
    web_link        TEXT,
    status          TEXT NOT NULL CHECK (status IN
        ('filtered', 'candidate', 'irrelevant', 'pending', 'processed', 'failed', 'ambiguous')),
    prefilter_score INTEGER,
    attempts        INTEGER NOT NULL DEFAULT 0,
    classification  TEXT,               -- ApplicationStatus
    category        TEXT,               -- job email category
    confidence      REAL,
    extraction      TEXT,               -- validated structured data (JSON)
    application_id  INTEGER REFERENCES job_applications (id) ON DELETE SET NULL,
    error           TEXT,
    first_seen_at   INTEGER NOT NULL,
    processed_at    INTEGER,
    PRIMARY KEY (provider, message_id)
);

INSERT INTO mail_messages_new (provider, message_id, thread_id, received_at, sender_domain, status,
        attempts, classification, extraction, application_id, error, first_seen_at, processed_at)
    SELECT 'google', message_id, thread_id, received_at, sender_domain, status, attempts,
        classification, extraction, application_id, error, first_seen_at, processed_at
    FROM mail_messages;
DROP TABLE mail_messages;
ALTER TABLE mail_messages_new RENAME TO mail_messages;
CREATE INDEX mail_messages_by_received ON mail_messages (received_at);
CREATE INDEX mail_messages_by_application ON mail_messages (application_id);

-- Mail threads / conversations known to belong to an application.
CREATE TABLE application_threads_new (
    provider       TEXT NOT NULL,
    thread_id      TEXT NOT NULL,
    application_id INTEGER NOT NULL REFERENCES job_applications (id) ON DELETE CASCADE,
    PRIMARY KEY (provider, thread_id)
);
INSERT INTO application_threads_new (provider, thread_id, application_id)
    SELECT 'google', thread_id, application_id FROM application_threads;
DROP TABLE application_threads;
ALTER TABLE application_threads_new RENAME TO application_threads;

-- The application timeline: every change with its source and confidence.
CREATE TABLE application_updates_new (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    application_id  INTEGER NOT NULL REFERENCES job_applications (id) ON DELETE CASCADE,
    source          TEXT NOT NULL,      -- gmail | outlook | google_calendar | outlook_calendar | user | assistant
    provider        TEXT,               -- google | microsoft (mail sources)
    message_id      TEXT,
    category        TEXT,
    confidence      REAL,
    status          TEXT NOT NULL,      -- status after this entry
    previous_status TEXT,
    change          TEXT NOT NULL,      -- e.g. "Application changed to Interview"
    summary         TEXT,
    occurred_at     INTEGER NOT NULL,
    created_at      INTEGER NOT NULL
);
INSERT INTO application_updates_new (id, application_id, source, provider, message_id, status,
        change, summary, occurred_at, created_at)
    SELECT id, application_id, 'gmail', 'google', message_id, status, 'Application update', summary,
        occurred_at, created_at
    FROM application_updates;
DROP TABLE application_updates;
ALTER TABLE application_updates_new RENAME TO application_updates;
CREATE UNIQUE INDEX application_updates_by_message
    ON application_updates (application_id, provider, message_id) WHERE message_id IS NOT NULL;
CREATE INDEX application_updates_by_time ON application_updates (occurred_at DESC);

-- Interviews: which mail provider they came from, the calendar that holds
-- their event, and what ReMa proposed or found.
ALTER TABLE interviews ADD COLUMN provider TEXT NOT NULL DEFAULT 'google';
ALTER TABLE interviews ADD COLUMN confidence REAL;
ALTER TABLE interviews ADD COLUMN fingerprint TEXT;
ALTER TABLE interviews ADD COLUMN calendar_provider TEXT;
-- none | proposed | conflict | created | declined | removed
ALTER TABLE interviews ADD COLUMN calendar_state TEXT NOT NULL DEFAULT 'none';
ALTER TABLE interviews ADD COLUMN conflicts TEXT;   -- JSON: [{title, startAt, endAt}]
-- Times an interview request proposes, checked against the calendar (JSON).
ALTER TABLE interviews ADD COLUMN proposed_slots TEXT;
UPDATE interviews SET calendar_provider = 'google', calendar_state = 'created'
    WHERE calendar_event_id IS NOT NULL;
DROP INDEX interviews_by_event;
CREATE UNIQUE INDEX interviews_by_event
    ON interviews (calendar_provider, calendar_event_id) WHERE calendar_event_id IS NOT NULL;
CREATE INDEX interviews_by_fingerprint ON interviews (fingerprint);

-- In-app notifications (also shown by the operating system).
CREATE TABLE notifications (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    kind           TEXT NOT NULL,
    title          TEXT NOT NULL,
    body           TEXT NOT NULL,
    application_id INTEGER REFERENCES job_applications (id) ON DELETE SET NULL,
    interview_id   INTEGER REFERENCES interviews (id) ON DELETE SET NULL,
    dedupe_key     TEXT UNIQUE,
    created_at     INTEGER NOT NULL,
    read_at        INTEGER
);
CREATE INDEX notifications_by_time ON notifications (created_at DESC);
