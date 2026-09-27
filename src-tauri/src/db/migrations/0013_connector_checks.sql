-- Production connectors: a connector records why its last sign-in check
-- failed as a stable code (USER_CANCELLED … PROVIDER_CONFIGURATION_ERROR),
-- next to the user-readable message. Codes never carry tokens.
ALTER TABLE connectors ADD COLUMN last_error_code TEXT;
