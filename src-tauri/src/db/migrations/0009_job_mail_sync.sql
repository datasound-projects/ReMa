-- Job Mail & Interview Sync: the current state the Applications page shows,
-- category names aligned with the classifier's schema, and built-in
-- scheduled tasks. Mail is tracked only by the built-in task from now on.

-- One concise line for the Applications table, the explicit rejection
-- reason (only if the email states one), the category of the email that set
-- the current status, and where the latest email came from.
ALTER TABLE job_applications ADD COLUMN latest_update TEXT;
ALTER TABLE job_applications ADD COLUMN rejection_reason TEXT;
ALTER TABLE job_applications ADD COLUMN status_category TEXT;
ALTER TABLE job_applications ADD COLUMN mail_provider TEXT;
ALTER TABLE job_applications ADD COLUMN mail_account TEXT;

UPDATE mail_messages SET category = 'application_confirmed' WHERE category = 'application_received';
UPDATE mail_messages SET category = 'needs_action' WHERE category = 'action_required';
UPDATE application_updates SET category = 'application_confirmed' WHERE category = 'application_received';
UPDATE application_updates SET category = 'needs_action' WHERE category = 'action_required';

-- A built-in task exists at most once ('job_mail_sync').
ALTER TABLE scheduled_tasks ADD COLUMN builtin TEXT;
CREATE UNIQUE INDEX scheduled_tasks_by_builtin ON scheduled_tasks (builtin) WHERE builtin IS NOT NULL;
UPDATE scheduled_tasks SET builtin = 'job_mail_sync', name = 'Job Mail & Interview Sync'
    WHERE id = (SELECT MIN(id) FROM scheduled_tasks WHERE kind LIKE '%"type":"job_applications"%');
-- Any other mail task stops: only the built-in task reads mail. It stays
-- listed, with its history, until the user deletes it.
UPDATE scheduled_tasks SET enabled = 0, next_run_at = NULL
    WHERE kind LIKE '%"type":"job_applications"%' AND builtin IS NULL;

-- Connectors no longer check mail on a schedule of their own.
UPDATE connectors SET background_sync = 0, next_sync_at = NULL;

-- When the email that set the current status was received. Kept apart from
-- `last_update_at` (the latest email of any kind): an older email processed
-- later never overrides a newer status.
ALTER TABLE job_applications ADD COLUMN status_at INTEGER;
UPDATE job_applications SET status_at = last_update_at;
