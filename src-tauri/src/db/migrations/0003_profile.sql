-- Profile, profile documents, and the explicit opt-in to share the profile
-- with a model (per conversation and per scheduled task).

-- Single row (id = 1). Lists with one value per entry live in their own
-- tables; `skills` is a JSON array of strings.
CREATE TABLE profile (
    id             INTEGER PRIMARY KEY CHECK (id = 1),
    first_name     TEXT NOT NULL DEFAULT '',
    last_name      TEXT NOT NULL DEFAULT '',
    email          TEXT NOT NULL DEFAULT '',
    phone          TEXT NOT NULL DEFAULT '',
    location       TEXT NOT NULL DEFAULT '',
    title          TEXT NOT NULL DEFAULT '',
    summary        TEXT NOT NULL DEFAULT '',
    skills         TEXT NOT NULL DEFAULT '[]',
    website        TEXT NOT NULL DEFAULT '',
    resume_website TEXT NOT NULL DEFAULT '',
    github         TEXT NOT NULL DEFAULT '',
    linkedin       TEXT NOT NULL DEFAULT '',
    updated_at     INTEGER NOT NULL
);

CREATE TABLE profile_experience (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    position    INTEGER NOT NULL,
    title       TEXT NOT NULL,
    company     TEXT NOT NULL,
    location    TEXT NOT NULL,
    start_date  TEXT NOT NULL,
    end_date    TEXT NOT NULL,
    current     INTEGER NOT NULL CHECK (current IN (0, 1)),
    description TEXT NOT NULL
);

CREATE TABLE profile_education (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    position    INTEGER NOT NULL,
    school      TEXT NOT NULL,
    degree      TEXT NOT NULL,
    field       TEXT NOT NULL,
    start_date  TEXT NOT NULL,
    end_date    TEXT NOT NULL,
    description TEXT NOT NULL
);

CREATE TABLE profile_languages (
    id       INTEGER PRIMARY KEY AUTOINCREMENT,
    position INTEGER NOT NULL,
    name     TEXT NOT NULL,
    level    TEXT NOT NULL
);

-- Links beyond website / resume website / GitHub / LinkedIn.
CREATE TABLE profile_links (
    id       INTEGER PRIMARY KEY AUTOINCREMENT,
    position INTEGER NOT NULL,
    label    TEXT NOT NULL,
    url      TEXT NOT NULL
);

-- Files live in <app data>/profile-documents/<file_name>; only metadata and
-- extracted text are stored here.
CREATE TABLE profile_documents (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    name          TEXT NOT NULL,
    kind          TEXT NOT NULL CHECK (kind IN ('cv', 'certificate', 'portfolio', 'other')),
    format        TEXT NOT NULL CHECK (format IN ('pdf', 'docx', 'text', 'markdown', 'png', 'jpeg')),
    original_name TEXT NOT NULL,
    file_name     TEXT NOT NULL UNIQUE,  -- generated: <random hex>.<ext>
    size          INTEGER NOT NULL,
    sha256        TEXT NOT NULL,
    text          TEXT,                  -- extracted text (NULL for images or unreadable files)
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL
);

CREATE TABLE profile_fields (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    position    INTEGER NOT NULL,
    label       TEXT NOT NULL,
    kind        TEXT NOT NULL CHECK (kind IN ('text', 'url', 'file')),
    value       TEXT NOT NULL DEFAULT '',
    document_id INTEGER REFERENCES profile_documents (id) ON DELETE SET NULL
);

ALTER TABLE conversations ADD COLUMN profile_context INTEGER NOT NULL DEFAULT 0 CHECK (profile_context IN (0, 1));
ALTER TABLE scheduled_tasks ADD COLUMN use_profile INTEGER NOT NULL DEFAULT 0 CHECK (use_profile IN (0, 1));
