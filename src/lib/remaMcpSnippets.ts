/**
 * How other MCP apps start ReMa MCP: Claude Code's `claude mcp add`,
 * Codex's `config.toml` and the `mcpServers` JSON of Claude Desktop,
 * Cursor and others.
 */
export type Launch = { command: string; args: string[] };

/**
 * A shell word: double-quoted when it has spaces or quotes. Backslashes stay
 * as they are, so Windows paths work in PowerShell, cmd and bash alike.
 */
const word = (s: string) => (/^[\w./:=@+\\-]+$/.test(s) ? s : `"${s.replace(/(["$`])/g, '\\$1')}"`);

export const claudeCodeCommand = (l: Launch) =>
  ['claude mcp add --scope user rema --', word(l.command), ...l.args.map(word)].join(' ');

export const codexToml = (l: Launch) =>
  `[mcp_servers.rema]\ncommand = ${JSON.stringify(l.command)}\nargs = ${JSON.stringify(l.args)}\n`;

export const mcpJson = (l: Launch) =>
  JSON.stringify({ mcpServers: { rema: { command: l.command, args: l.args } } }, null, 2);
