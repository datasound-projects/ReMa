-- Portfolio Studio: cover letters next to CVs, the user's design choices on
-- top of a template, and the Profile document a rebuilt CV came from.
ALTER TABLE portfolio_documents ADD COLUMN kind TEXT NOT NULL DEFAULT 'cv';
ALTER TABLE portfolio_documents ADD COLUMN style TEXT NOT NULL DEFAULT '{}';
ALTER TABLE portfolio_documents ADD COLUMN letter TEXT NOT NULL DEFAULT '{}';
ALTER TABLE portfolio_documents ADD COLUMN source_document_id INTEGER;
