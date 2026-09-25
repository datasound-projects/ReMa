-- How each provider is connected: the transport and its authentication.
-- 'api_key'         the provider's HTTPS API (key in the OS credential store,
--                   or none for local endpoints)
-- 'chatgpt_account' OpenAI through the official Codex runtime (ChatGPT sign-in)
-- 'claude_console'  the Anthropic API with a Claude Console sign-in managed by
--                   the official Anthropic CLI
ALTER TABLE providers ADD COLUMN connection TEXT NOT NULL DEFAULT 'api_key'
    CHECK (connection IN ('api_key', 'chatgpt_account', 'claude_console'));

-- Who is signed in for account connections ("ana@example.com · Plus").
-- Display text only: credentials stay with the provider's runtime.
ALTER TABLE providers ADD COLUMN account_label TEXT;
