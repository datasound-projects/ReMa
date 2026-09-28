-- The user's own contacts, imported from their LinkedIn data export
-- (Connections.csv) or vCard files (e.g. saved from XING). Only what
-- Network Connect matches on is kept: no e-mail addresses or phone numbers.
CREATE TABLE network_contacts (
    id INTEGER PRIMARY KEY,
    -- 'linkedin_export' or 'vcard'
    source TEXT NOT NULL,
    name TEXT NOT NULL,
    company TEXT NOT NULL DEFAULT '',
    position TEXT NOT NULL DEFAULT '',
    profile_url TEXT,
    -- As the export states it ("12 Mar 2021"), for display only.
    connected_on TEXT,
    imported_at INTEGER NOT NULL,
    UNIQUE (source, name, company)
);
