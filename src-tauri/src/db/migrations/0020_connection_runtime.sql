-- Connection runtime: when an account's grant was last renewed (a renewal
-- is made only for a concrete reason: an expired access token, a rejected
-- request, or a grant unused so long the provider would expire it).
ALTER TABLE connector_accounts ADD COLUMN last_refreshed_at INTEGER;
