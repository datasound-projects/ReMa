-- ReMa Business (Appendix B): a Business Profile with versioned offers,
-- research runs and reproducible fit assessments, one commercial pipeline
-- (separate from job Applications), local drafts, and Go-to-Market plans
-- with experiments. Nothing here holds a token, a provider session or
-- member data a provider restricts.

CREATE TABLE business_profile (
    id             INTEGER PRIMARY KEY CHECK (id = 1),
    business_name  TEXT NOT NULL DEFAULT '',
    website        TEXT NOT NULL DEFAULT '',
    service_area   TEXT NOT NULL DEFAULT '',
    languages      TEXT NOT NULL DEFAULT '[]',
    capacity       TEXT NOT NULL DEFAULT '',
    availability   TEXT NOT NULL DEFAULT '',
    constraints    TEXT NOT NULL DEFAULT '',
    updated_at     INTEGER NOT NULL
);

CREATE TABLE business_offers (
    id               TEXT PRIMARY KEY,
    name             TEXT NOT NULL,
    kind             TEXT NOT NULL CHECK (kind IN ('service', 'digital_product', 'hybrid')),
    -- The newest reviewed version (NULL until the first review).
    current_version  INTEGER,
    -- The draft being edited or reviewed (JSON), never used by research.
    draft            TEXT,
    archived         INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0, 1)),
    idempotency_key  TEXT UNIQUE,
    created_at       INTEGER NOT NULL,
    updated_at       INTEGER NOT NULL,
    revision         INTEGER NOT NULL DEFAULT 0
);

-- Immutable: research and experiments keep the version they used.
CREATE TABLE business_offer_versions (
    offer_id     TEXT NOT NULL REFERENCES business_offers(id) ON DELETE CASCADE,
    version      INTEGER NOT NULL,
    content      TEXT NOT NULL,
    reviewed_at  INTEGER NOT NULL,
    PRIMARY KEY (offer_id, version)
);

CREATE TABLE business_research_runs (
    id              TEXT PRIMARY KEY,
    kind            TEXT NOT NULL CHECK (kind IN ('offer_ingest', 'clients', 'contracts', 'gtm')),
    offer_id        TEXT,
    offer_version   INTEGER,
    query           TEXT NOT NULL,
    criteria        TEXT NOT NULL DEFAULT '{}',
    scoring_policy  TEXT,
    data_policy     TEXT NOT NULL,
    model           TEXT,
    status          TEXT NOT NULL CHECK (status IN ('queued', 'running', 'complete', 'partial',
                        'no_verified_matches', 'needs_review', 'capability_unavailable', 'offline',
                        'failed', 'cancelled')),
    result          TEXT,
    sources         TEXT NOT NULL DEFAULT '[]',
    failures        TEXT NOT NULL DEFAULT '[]',
    started_at      INTEGER NOT NULL,
    finished_at     INTEGER
);
CREATE INDEX business_research_runs_started ON business_research_runs (started_at DESC);

CREATE TABLE business_opportunities (
    id                           TEXT PRIMARY KEY,
    kind                         TEXT NOT NULL CHECK (kind IN ('product', 'service', 'contract')),
    name                         TEXT NOT NULL,
    company_key                  TEXT,
    company_name                 TEXT,
    offer_id                     TEXT,
    offer_version                INTEGER,
    canonical_job_id             TEXT,
    source_url                   TEXT,
    use_case                     TEXT NOT NULL DEFAULT '',
    stage                        TEXT NOT NULL CHECK (stage IN ('new_lead', 'qualified', 'contacted',
                                     'discussion', 'proposal', 'won', 'lost')),
    archived                     INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0, 1)),
    do_not_contact               INTEGER NOT NULL DEFAULT 0 CHECK (do_not_contact IN (0, 1)),
    contacts                     TEXT NOT NULL DEFAULT '[]',
    -- Contacts the user removed: later research does not add them again.
    removed_contacts             TEXT NOT NULL DEFAULT '[]',
    evidence                     TEXT NOT NULL DEFAULT '[]',
    amount                       TEXT,
    contract                     TEXT,
    listing_status               TEXT,
    next_step                    TEXT NOT NULL DEFAULT '',
    notes                        TEXT NOT NULL DEFAULT '',
    idempotency_key              TEXT NOT NULL UNIQUE,
    created_at                   INTEGER NOT NULL,
    updated_at                   INTEGER NOT NULL,
    last_researched_at           INTEGER,
    last_commercial_activity_at  INTEGER,
    revision                     INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX business_opportunities_stage ON business_opportunities (stage, updated_at DESC);

CREATE TABLE business_assessments (
    id              TEXT PRIMARY KEY,
    run_id          TEXT REFERENCES business_research_runs(id) ON DELETE SET NULL,
    opportunity_id  TEXT REFERENCES business_opportunities(id) ON DELETE CASCADE,
    company_key     TEXT NOT NULL,
    company_name    TEXT NOT NULL,
    offer_id        TEXT,
    offer_version   INTEGER,
    policy_version  TEXT NOT NULL,
    content         TEXT NOT NULL,
    coverage        REAL NOT NULL,
    score           REAL,
    created_at      INTEGER NOT NULL
);
CREATE INDEX business_assessments_opportunity ON business_assessments (opportunity_id);

-- Actual commercial activity, as the user reports it. A draft, a copied
-- message or an opened link is never an activity.
CREATE TABLE business_activities (
    id               TEXT PRIMARY KEY,
    opportunity_id   TEXT NOT NULL REFERENCES business_opportunities(id) ON DELETE CASCADE,
    kind             TEXT NOT NULL CHECK (kind IN ('contact', 'reply', 'positive_reply',
                         'meeting_held', 'proposal_sent', 'won', 'lost', 'stage_change', 'note')),
    person           TEXT,
    occurred_at      INTEGER NOT NULL,
    recorded_at      INTEGER NOT NULL,
    source           TEXT NOT NULL CHECK (source IN ('user_reported')),
    detail           TEXT,
    from_stage       TEXT,
    to_stage         TEXT,
    experiment_id    TEXT,
    variant          TEXT,
    idempotency_key  TEXT NOT NULL UNIQUE
);
CREATE INDEX business_activities_opportunity ON business_activities (opportunity_id, occurred_at);

-- "Do not contact": research and drafting cannot undo it.
CREATE TABLE business_suppressions (
    id          TEXT PRIMARY KEY,
    scope       TEXT NOT NULL CHECK (scope IN ('company', 'person')),
    key         TEXT NOT NULL,
    label       TEXT NOT NULL,
    reason      TEXT,
    created_at  INTEGER NOT NULL,
    UNIQUE (scope, key)
);

-- Local drafts only: there is no sending.
CREATE TABLE business_drafts (
    id              TEXT PRIMARY KEY,
    opportunity_id  TEXT REFERENCES business_opportunities(id) ON DELETE CASCADE,
    plan_id         TEXT,
    experiment_id   TEXT,
    variant         TEXT,
    offer_id        TEXT,
    offer_version   INTEGER,
    recipient       TEXT NOT NULL,
    channel         TEXT NOT NULL,
    subject         TEXT,
    body            TEXT NOT NULL,
    evidence        TEXT NOT NULL DEFAULT '[]',
    idempotency_key TEXT UNIQUE,
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL,
    revision        INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE gtm_plans (
    id             TEXT PRIMARY KEY,
    offer_id       TEXT NOT NULL,
    offer_version  INTEGER NOT NULL,
    name           TEXT NOT NULL,
    geography      TEXT NOT NULL DEFAULT '',
    content        TEXT NOT NULL,
    idempotency_key TEXT UNIQUE,
    created_at     INTEGER NOT NULL,
    updated_at     INTEGER NOT NULL,
    revision       INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE gtm_experiments (
    id                 TEXT PRIMARY KEY,
    plan_id            TEXT NOT NULL REFERENCES gtm_plans(id) ON DELETE CASCADE,
    offer_id           TEXT NOT NULL,
    offer_version      INTEGER NOT NULL,
    segment_id         TEXT,
    version            INTEGER NOT NULL DEFAULT 1,
    content            TEXT NOT NULL,
    status             TEXT NOT NULL CHECK (status IN ('draft', 'planned', 'running', 'paused',
                           'completed', 'cancelled')),
    frozen_at          INTEGER,
    idempotency_key    TEXT UNIQUE,
    created_at         INTEGER NOT NULL,
    updated_at         INTEGER NOT NULL,
    revision           INTEGER NOT NULL DEFAULT 0
);

-- What was removed and when, without the removed data (B29 tombstones).
CREATE TABLE business_redactions (
    id              TEXT PRIMARY KEY,
    opportunity_id  TEXT,
    kind            TEXT NOT NULL,
    detail          TEXT NOT NULL,
    created_at      INTEGER NOT NULL
);
