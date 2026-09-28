// @vitest-environment jsdom
import '../../test/dom';

import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { McpImportCandidate, McpImportSource } from '../../services/mcpService';
import { McpImportDialog } from './McpImportDialog';

const mocks = vi.hoisted(() => ({
  mcpImportSources: vi.fn(),
  previewMcpImport: vi.fn(),
  importMcpServers: vi.fn(),
}));
vi.mock('../../services/mcpService', () => mocks);

const candidate = (name: string, extra: Partial<McpImportCandidate> = {}): McpImportCandidate => ({
  name,
  transport: 'stdio',
  summary: `npx -y ${name}-mcp`,
  envNames: [],
  alreadyAdded: false,
  problem: null,
  disabled: false,
  ...extra,
});

const claudeDesktop: McpImportSource = {
  app: 'claude_desktop',
  label: 'Claude Desktop',
  path: '/Users/ana/Library/Application Support/Claude/claude_desktop_config.json',
  servers: [
    candidate('github', { envNames: ['GITHUB_PERSONAL_ACCESS_TOKEN'] }),
    candidate('files', { alreadyAdded: true }),
    candidate('shell', { transport: null, problem: 'It starts through a shell; add it with the program itself.' }),
  ],
};

beforeEach(() => {
  vi.clearAllMocks();
  mocks.importMcpServers.mockResolvedValue({ added: [{ name: 'github' }], skipped: [] });
});

describe('Settings → MCP → Import from other apps', () => {
  it('lists the servers found in Claude Desktop and adds the chosen ones', async () => {
    mocks.mcpImportSources.mockResolvedValue([claudeDesktop]);
    render(<McpImportDialog onClose={() => {}} />);

    const github = (await screen.findByRole('checkbox', { name: 'github' })) as HTMLInputElement;
    expect(github.checked).toBe(true);
    expect(screen.getByText('Variables: GITHUB_PERSONAL_ACCESS_TOKEN')).toBeTruthy();
    expect((screen.getByRole('checkbox', { name: 'files' }) as HTMLInputElement).disabled).toBe(true);
    expect(screen.getByText('Already in ReMa.')).toBeTruthy();
    expect((screen.getByRole('checkbox', { name: 'shell' }) as HTMLInputElement).disabled).toBe(true);
    expect(screen.getByText(/through a shell/)).toBeTruthy();

    fireEvent.click(screen.getByRole('button', { name: 'Add 1 server' }));
    await waitFor(() =>
      expect(mocks.importMcpServers).toHaveBeenCalledWith({ app: 'claude_desktop', json: null, names: ['github'] }),
    );
    expect(await screen.findByText('Added github.')).toBeTruthy();
  });

  it('adds servers from pasted JSON after showing them', async () => {
    mocks.mcpImportSources.mockResolvedValue([]);
    mocks.previewMcpImport.mockResolvedValue([candidate('notes')]);
    render(<McpImportDialog onClose={() => {}} />);

    expect(await screen.findByText(/No MCP servers were found in Claude Desktop/)).toBeTruthy();
    const json = '{"mcpServers":{"notes":{"command":"npx","args":["notes-mcp"]}}}';
    fireEvent.change(screen.getByLabelText('MCP configuration'), { target: { value: json } });
    fireEvent.click(screen.getByRole('button', { name: 'Show servers' }));
    expect(await screen.findByRole('checkbox', { name: 'notes' })).toBeTruthy();
    expect(mocks.previewMcpImport).toHaveBeenCalledWith(json);

    fireEvent.click(screen.getByRole('button', { name: 'Add 1 server' }));
    await waitFor(() =>
      expect(mocks.importMcpServers).toHaveBeenCalledWith({ app: null, json, names: ['notes'] }),
    );
  });
});
