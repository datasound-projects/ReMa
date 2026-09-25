-- Google Workspace + job-application intelligence.
-- OAuth tokens are never stored here (they live in the OS credential store).

-- Task type and its configuration, e.g. {"type":"prompt"} or
-- {"type":"job_applications","lookbackDays":14,...}.
ALTER TABLE scheduled_tasks ADD COLUMN kind TEXT NOT NULL DEFAULT '{"type":"prompt"}';

-- Structured result of a run (JSON), e.g. the job-application report.
ALTER TABLE task_executions ADD COLUMN report TEXT;

-- One job application, built from one or more emails.
CREATE TABLE job_applications (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    company         TEXT NOT NULL,
    company_key     TEXT NOT NULL,      -- normalized for matching
    role            TEXT,
    role_key        TEXT,
    reference       TEXT,               -- job / application reference number
    sender_domain   TEXT,
    status          TEXT NOT NULL,      -- ApplicationStatus (validated in Rust)
    requires_action INTEGER NOT NULL DEFAULT 0 CHECK (requires_action IN (0, 1)),
    next_action     TEXT,
    last_update_at  INTEGER NOT NULL,   -- received time of the latest email
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL
);

CREATE INDEX job_applications_by_company ON job_applications (company_key);
CREATE INDEX job_applications_by_update ON job_applications (last_update_at DESC);

-- Gmail threads known to belong to an application.
CREATE TABLE application_threads (
    thread_id      TEXT PRIMARY KEY,
    application_id INTEGER NOT NULL REFERENCES job_applications (id) ON DELETE CASCADE
);

-- Status history of an application (one row per relevant email).
CREATE TABLE application_updates (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    application_id INTEGER NOT NULL REFERENCES job_applications (id) ON DELETE CASCADE,
    message_id     TEXT NOT NULL,
    status         TEXT NOT NULL,
    summary        TEXT,
    occurred_at    INTEGER NOT NULL,
    created_at     INTEGER NOT NULL,
    UNIQUE (application_id, message_id)
);

-- Every Gmail message ReMa has looked at, so runs are incremental.
-- No message bodies are stored.
CREATE TABLE mail_messages (
    message_id     TEXT PRIMARY KEY,
    thread_id      TEXT NOT NULL,
    received_at    INTEGER NOT NULL,
    sender_domain  TEXT,
    status         TEXT NOT NULL CHECK (status IN ('irrelevant', 'pending', 'processed', 'failed')),
    attempts       INTEGER NOT NULL DEFAULT 0,
    classification TEXT,                -- ApplicationStatus
    extraction     TEXT,                -- validated structured data (JSON)
    application_id INTEGER REFERENCES job_applications (id) ON DELETE SET NULL,
    error          TEXT,
    first_seen_at  INTEGER NOT NULL,
    processed_at   INTEGER
);

CREATE INDEX mail_messages_by_received ON mail_messages (received_at);

-- Interviews detected in email, and their link to Google Calendar.
CREATE TABLE interviews (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    application_id    INTEGER NOT NULL REFERENCES job_applications (id) ON DELETE CASCADE,
    source_thread_id  TEXT NOT NULL,
    source_message_id TEXT NOT NULL,
    state             TEXT NOT NULL CHECK (state IN ('proposed', 'confirmed', 'needs_review', 'cancelled')),
    interview_type    TEXT,
    start_at          INTEGER,
    end_at            INTEGER,
    timezone          TEXT,
    location          TEXT,
    meeting_url       TEXT,
    participants      TEXT,             -- JSON array of names
    review_reason     TEXT,             -- why it needs a human
    calendar_event_id TEXT,             -- Google Calendar event ReMa manages
    calendar_hash     TEXT,             -- content last written to Calendar
    calendar_synced_at INTEGER,
    created_at        INTEGER NOT NULL,
    updated_at        INTEGER NOT NULL
);

CREATE INDEX interviews_by_application ON interviews (application_id);
CREATE UNIQUE INDEX interviews_by_event ON interviews (calendar_event_id) WHERE calendar_event_id IS NOT NULL;

-- What happened to an interview over time (rescheduled, cancelled, synced).
CREATE TABLE interview_history (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    interview_id INTEGER NOT NULL REFERENCES interviews (id) ON DELETE CASCADE,
    change       TEXT NOT NULL,
    details      TEXT,
    message_id   TEXT,
    created_at   INTEGER NOT NULL
);
