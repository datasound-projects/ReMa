-- Run history: every execution of a scheduled task is a run that keeps the
-- task as it was when the run was created (a snapshot later edits never
-- change), what took part, its progress, its outputs (by reference) and a
-- safe error. A run is created (queued) before the task executes.

-- The new statuses (queued, cancelled) need a new CHECK, so the table is
-- rebuilt. Run ids are never reused: Analytics keys task runs by id, so the
-- AUTOINCREMENT counter moves over with the rows.
CREATE TABLE task_executions_new (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id        INTEGER NOT NULL REFERENCES scheduled_tasks (id) ON DELETE CASCADE,
    trigger        TEXT NOT NULL CHECK (trigger IN ('scheduled', 'manual')),
    status         TEXT NOT NULL
                   CHECK (status IN ('queued', 'running', 'succeeded', 'failed', 'cancelled')),
    scheduled_for  INTEGER,            -- the occurrence a scheduled run belongs to
    queued_at      INTEGER NOT NULL,   -- the run was created
    started_at     INTEGER,            -- execution began
    finished_at    INTEGER,
    -- The task when the run was created. NULL in runs recorded before
    -- snapshots were kept (their prompt and model were always kept).
    task_name      TEXT,
    kind           TEXT,               -- TaskKind (JSON)
    schedule       TEXT,               -- ScheduleSnapshot (JSON)
    use_profile    INTEGER CHECK (use_profile IN (0, 1)),
    provider_id    TEXT NOT NULL,
    model_id       TEXT NOT NULL,
    prompt         TEXT NOT NULL,
    -- What took part (Profile, connected services and web search, by name):
    -- RunContext (JSON), never credentials.
    context        TEXT,
    result         TEXT,
    report         TEXT,
    error          TEXT,               -- safe, user-facing message
    error_category TEXT
);

INSERT INTO task_executions_new
    (id, task_id, trigger, status, scheduled_for, queued_at, started_at, finished_at,
     provider_id, model_id, prompt, result, report, error, error_category)
SELECT id, task_id, trigger,
       CASE WHEN status = 'failed' AND error = 'The run was cancelled.' THEN 'cancelled'
            ELSE status END,
       scheduled_for, started_at, started_at, finished_at,
       provider_id, model_id, prompt, result, report, error,
       CASE WHEN status <> 'failed' THEN NULL
            WHEN error LIKE 'Interrupted:%' THEN 'interrupted'
            WHEN error = 'The run timed out.' THEN 'timeout'
            WHEN error = 'The run was cancelled.' THEN 'cancelled'
            WHEN error IN ('The model declined to answer this prompt.',
                           'The model returned an empty response.') THEN 'model'
            ELSE NULL END
FROM task_executions;

DELETE FROM sqlite_sequence WHERE name = 'task_executions_new';
INSERT INTO sqlite_sequence (name, seq)
    SELECT 'task_executions_new', seq FROM sqlite_sequence WHERE name = 'task_executions';

DROP TABLE task_executions;
ALTER TABLE task_executions_new RENAME TO task_executions;

-- Newest first per task; unfinished runs for start-up reconciliation.
CREATE INDEX task_executions_by_task ON task_executions (task_id, id DESC);
CREATE INDEX task_executions_active ON task_executions (status)
    WHERE status IN ('queued', 'running');

-- A run's stages as they happened ("Searched jobs", "Updated 3
-- applications"): concise activity, never model reasoning.
CREATE TABLE task_run_events (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    execution_id INTEGER NOT NULL REFERENCES task_executions (id) ON DELETE CASCADE,
    stage        TEXT NOT NULL,      -- stable key within the run
    label        TEXT NOT NULL,
    status       TEXT NOT NULL
                 CHECK (status IN ('pending', 'running', 'completed', 'failed', 'skipped')),
    started_at   INTEGER,
    updated_at   INTEGER NOT NULL,
    UNIQUE (execution_id, stage)
);

-- What a run produced besides its text, as references to where it lives
-- (the run's report, an Analytics job search, an application): never the
-- content itself.
CREATE TABLE task_run_outputs (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    execution_id INTEGER NOT NULL REFERENCES task_executions (id) ON DELETE CASCADE,
    kind         TEXT NOT NULL,
    title        TEXT NOT NULL,
    reference    TEXT NOT NULL,      -- RunOutputRef (JSON)
    created_at   INTEGER NOT NULL
);

CREATE INDEX task_run_outputs_by_run ON task_run_outputs (execution_id);

-- Outputs earlier runs already had: the report of a Job Mail & Interview
-- Sync run, and the job searches Analytics made from a run's result.
INSERT INTO task_run_outputs (execution_id, kind, title, reference, created_at)
SELECT id, 'application_watch', 'Application Watch', '{"type":"run_report"}',
       COALESCE(finished_at, started_at)
FROM task_executions WHERE report IS NOT NULL;

INSERT INTO task_run_outputs (execution_id, kind, title, reference, created_at)
SELECT r.execution_id, 'job_search_results', 'Job Search Results',
       json_object('type', 'job_search', 'id', r.id), r.created_at
FROM job_search_runs r JOIN task_executions e ON e.id = r.execution_id;
