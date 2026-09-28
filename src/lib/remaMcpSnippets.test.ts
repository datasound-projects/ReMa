import { describe, expect, it } from 'vitest';

import { claudeCodeCommand, codexToml, mcpJson } from './remaMcpSnippets';

describe('ReMa MCP in other apps', () => {
  const mac = { command: '/Applications/ReMa.app/Contents/MacOS/ReMa', args: ['mcp'] };
  const windows = { command: 'C:\\Program Files\\ReMa\\ReMa.exe', args: ['mcp', '--data-dir', 'D:\\Re Ma'] };

  it('gives Claude Code a command that survives spaces in paths', () => {
    expect(claudeCodeCommand(mac)).toBe('claude mcp add --scope user rema -- /Applications/ReMa.app/Contents/MacOS/ReMa mcp');
    expect(claudeCodeCommand(windows)).toBe(
      'claude mcp add --scope user rema -- "C:\\Program Files\\ReMa\\ReMa.exe" mcp --data-dir "D:\\Re Ma"',
    );
  });

  it('writes Codex TOML and mcpServers JSON that parse back to the same program', () => {
    expect(codexToml(windows)).toBe(
      '[mcp_servers.rema]\ncommand = "C:\\\\Program Files\\\\ReMa\\\\ReMa.exe"\nargs = ["mcp","--data-dir","D:\\\\Re Ma"]\n',
    );
    expect(JSON.parse(mcpJson(mac))).toEqual({ mcpServers: { rema: mac } });
  });
});
