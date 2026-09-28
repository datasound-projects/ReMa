-- Why a provider ended a connection that now needs a reconnect: Google's
-- 7-day limit for apps in Testing, access revoked, unused too long, a
-- security policy, a blocked account or a token that cannot be renewed.
-- NULL: connected, or not known.
ALTER TABLE connector_accounts ADD COLUMN status_cause TEXT;
