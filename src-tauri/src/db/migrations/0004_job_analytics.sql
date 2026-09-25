-- Job analytics: deduplicated job records found by ReMa's job searches.
-- Missing values stay NULL; nothing is filled with guesses or zeros.

CREATE TABLE jobs (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    title            TEXT NOT NULL,          -- as stated, without gender markers
    normalized_title TEXT NOT NULL,          -- role without seniority words
    company          TEXT,
    company_key      TEXT,                   -- normalized for matching
    location         TEXT,                   -- as stated
    city             TEXT,
    country          TEXT,
    work_mode        TEXT CHECK (work_mode IN ('remote', 'hybrid', 'onsite')),
    employment_type  TEXT CHECK (employment_type IN ('full_time', 'part_time', 'contract', 'freelance', 'temporary', 'internship')),
    seniority        TEXT CHECK (seniority IN ('intern', 'entry', 'mid', 'senior', 'lead', 'executive')),
    salary_text      TEXT,                   -- as stated
    salary_min       REAL,
    salary_max       REAL,
    salary_currency  TEXT,                   -- ISO 4217
    salary_period    TEXT CHECK (salary_period IN ('year', 'month', 'week', 'day', 'hour')),
    date_posted      INTEGER,                -- UTC midnight of the stated posting day
    date_discovered  INTEGER NOT NULL,       -- first search that found it
    last_seen        INTEGER NOT NULL,       -- latest search that found it
    source           TEXT,                   -- job board or site
    source_url       TEXT,
    external_ref     TEXT,
    description      TEXT,
    benefits         TEXT NOT NULL DEFAULT '[]',  -- JSON array of strings
    details_status   TEXT NOT NULL DEFAULT 'pending' CHECK (details_status IN ('pending', 'done', 'failed', 'skipped')),
    details_note     TEXT,                   -- why details are missing, if they are
    details_attempts INTEGER NOT NULL DEFAULT 0,
    created_at       INTEGER NOT NULL,
    updated_at       INTEGER NOT NULL
);

CREATE INDEX jobs_by_company ON jobs (company_key);
CREATE INDEX jobs_by_role ON jobs (normalized_title);
CREATE INDEX jobs_by_place ON jobs (country, city);
CREATE INDEX jobs_by_posted ON jobs (date_posted);
CREATE INDEX jobs_by_discovered ON jobs (date_discovered);
CREATE INDEX jobs_by_details ON jobs (details_status);

-- Deterministic evidence identifying a job: 'id:<board>:<id>', 'url:<canonical>',
-- 'ref:<company>|<reference>', 'desc:<company>|<hash>', 'ctl:<company>|<title>|<place>'.
CREATE TABLE job_keys (
    key    TEXT PRIMARY KEY,
    job_id INTEGER NOT NULL REFERENCES jobs (id) ON DELETE CASCADE
);

CREATE INDEX job_keys_by_job ON job_keys (job_id);

-- One execution of a job search (chat answer, scheduled task run, …).
-- Runs are never overwritten by later searches.
CREATE TABLE job_search_runs (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    origin          TEXT NOT NULL UNIQUE,    -- 'chat:<message id>', 'task:<execution id>', …
    title           TEXT NOT NULL,
    query           TEXT NOT NULL,
    source          TEXT NOT NULL CHECK (source IN ('chat', 'task', 'manual', 'tool')),
    task_id         INTEGER REFERENCES scheduled_tasks (id) ON DELETE SET NULL,
    conversation_id INTEGER REFERENCES conversations (id) ON DELETE SET NULL,
    message_id      INTEGER REFERENCES messages (id) ON DELETE SET NULL,
    execution_id    INTEGER REFERENCES task_executions (id) ON DELETE SET NULL,
    result_count    INTEGER NOT NULL,
    created_at      INTEGER NOT NULL
);

CREATE INDEX job_search_runs_by_created ON job_search_runs (created_at DESC);
CREATE INDEX job_search_runs_by_task ON job_search_runs (task_id);
CREATE INDEX job_search_runs_by_message ON job_search_runs (message_id);

-- Which jobs a run found. A job found by several runs is stored once.
CREATE TABLE job_search_run_jobs (
    run_id   INTEGER NOT NULL REFERENCES job_search_runs (id) ON DELETE CASCADE,
    job_id   INTEGER NOT NULL REFERENCES jobs (id) ON DELETE CASCADE,
    position INTEGER NOT NULL,               -- order in the result
    raw      TEXT NOT NULL,                  -- the listing as this run reported it (JSON)
    PRIMARY KEY (run_id, job_id)
);

CREATE INDEX job_search_run_jobs_by_job ON job_search_run_jobs (job_id);

-- Normalized requirements: skills, technologies, experience, education,
-- certifications, languages and soft skills (`kind` + `category`).
CREATE TABLE job_requirements (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    job_id     INTEGER NOT NULL REFERENCES jobs (id) ON DELETE CASCADE,
    kind       TEXT NOT NULL CHECK (kind IN ('skill', 'experience', 'education', 'certification', 'language', 'soft_skill', 'other')),
    category   TEXT NOT NULL,
    name       TEXT NOT NULL,                -- canonical, e.g. 'Kubernetes'
    original   TEXT NOT NULL,                -- wording in the source
    importance TEXT NOT NULL CHECK (importance IN ('required', 'preferred')),
    years      INTEGER,                      -- experience: minimum years
    level      TEXT,                         -- language level or degree
    source     TEXT NOT NULL CHECK (source IN ('search', 'description', 'model')),
    UNIQUE (job_id, kind, name)
);

CREATE INDEX job_requirements_by_name ON job_requirements (name);

-- The dashboard's working state (scope, filters, ranking, options).
CREATE TABLE analytics_preferences (
    id         INTEGER PRIMARY KEY CHECK (id = 1),
    state      TEXT NOT NULL,                -- JSON `AnalyticsPreferences`
    updated_at INTEGER NOT NULL
);

-- Learning-resource research, with the deterministic snapshot it answered.
CREATE TABLE learning_recommendations (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    dataset_key     TEXT NOT NULL,           -- hash of selection + criteria
    dataset_label   TEXT NOT NULL,
    dataset         TEXT NOT NULL,           -- JSON: query and criteria
    job_count       INTEGER NOT NULL,
    gap_snapshot    TEXT NOT NULL,           -- JSON: the prioritized gaps
    recommendations TEXT NOT NULL DEFAULT '[]',  -- JSON: validated recommendations
    status          TEXT NOT NULL CHECK (status IN ('running', 'done', 'failed')),
    error           TEXT,
    provider_id     TEXT,
    model_id        TEXT,
    generated_at    INTEGER NOT NULL
);

CREATE INDEX learning_recommendations_by_dataset ON learning_recommendations (dataset_key, generated_at DESC);
