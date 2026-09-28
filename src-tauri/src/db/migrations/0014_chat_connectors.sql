-- Connectors a conversation may use (the composer's toggles), as a JSON
-- array of ids ("gmail", "google_calendar", "outlook_mail",
-- "outlook_calendar", "applications"). NULL: every connected one, the
-- default for new and earlier chats.
ALTER TABLE conversations ADD COLUMN connectors TEXT;
