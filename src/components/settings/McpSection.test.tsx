// @vitest-environment jsdom
import '../../test/dom';

import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { McpServer } from '../../services/mcpService';
import { McpSection } from './McpSection';

const mocks = vi.hoisted(() => ({
  listMcpServers: vi.fn(),
  saveMcpServer: vi.fn(),
  deleteMcpServer: vi.fn(),
  setMcpServerEnabled: vi.fn(),
  testMcpServer: vi.fn(),
  connectMcpServer: vi.fn(),
  disconnectMcpServer: vi.fn(),
  signInMcpServer: vi.fn(),
  cancelMcpSignIn: vi.fn(),
  signOutMcpServer: vi.fn(),
}));

vi.mock('../../services/mcpService', () => mocks);

const server: McpServer = {
  id: 3,
  name: 'Local Files',
  transport: 'stdio',
  command: 'npx',
  args: ['-y', '@modelcontextprotocol/server-filesystem', '/Users/me/Documents'],
  envNames: ['API_KEY'],
  cwd: '',
  url: '',
  auth: 'none',
  headerName: '',
  hasSecret: false,
  enabled: false,
  status: { state: 'disabled', message: null, tools: [], protocolVersion: null, serverInfo: null },
  createdAt: 0,
  updatedAt: 0,
};

beforeEach(() => {
  vi.clearAllMocks();
  mocks.listMcpServers.mockResolvedValue([server]);
});

describe('Settings → MCP', () => {
  it('explains MCP in one short sentence on hover', async () => {
    render(<McpSection />);
    expect(screen.getByRole('heading', { name: /MCP/ })).toBeTruthy();
    expect(screen.getByText('Model Context Protocol')).toBeTruthy();
    const help = screen.getByRole('button', { name: 'What is MCP?' });
    fireEvent.focus(help);
    expect(screen.getByRole('tooltip').textContent).toBe(
      'MCP lets ReMa connect to external tools and data sources that AI models can use when you allow them.',
    );
  });

  it('shows each server with type, status, switch, edit, test and remove', async () => {
    mocks.setMcpServerEnabled.mockResolvedValue({ ...server, enabled: true });
    mocks.testMcpServer.mockResolvedValue({ ok: true, message: 'Connected (MCP 2026-07-28). 2 tools available.', tools: [] });
    mocks.deleteMcpServer.mockResolvedValue(null);
    render(<McpSection />);
    expect(await screen.findByText('Local Files')).toBeTruthy();
    expect(screen.getByText(/Local program/)).toBeTruthy();
    expect(screen.getByText('Off')).toBeTruthy();

    fireEvent.click(screen.getByRole('switch', { name: 'Turn on Local Files' }));
    await waitFor(() => expect(mocks.setMcpServerEnabled).toHaveBeenCalledWith(3, true));

    fireEvent.click(screen.getByRole('button', { name: 'Test' }));
    expect(await screen.findByText('Connected (MCP 2026-07-28). 2 tools available.')).toBeTruthy();
    // Stored secrets stay in the keychain: the test sends "unchanged".
    expect(mocks.testMcpServer).toHaveBeenCalledWith(
      3,
      expect.objectContaining({ env: [{ name: 'API_KEY', value: null }], secret: null }),
    );

    fireEvent.click(screen.getByRole('button', { name: 'Remove' }));
    fireEvent.click(within(screen.getByRole('group', { name: 'Confirm removal' })).getByRole('button', { name: 'Remove' }));
    await waitFor(() => expect(mocks.deleteMcpServer).toHaveBeenCalledWith(3));
  });

  it('adds a server with only the fields of its connection type', async () => {
    mocks.saveMcpServer.mockResolvedValue({ ...server, id: 4 });
    render(<McpSection />);
    fireEvent.click(await screen.findByRole('button', { name: 'Add MCP server' }));
    const dialog = await screen.findByRole('dialog', { name: 'Add MCP server' });
    expect(within(dialog).getByText(/New servers start turned off/)).toBeTruthy();
    expect(within(dialog).getByLabelText('Program')).toBeTruthy();
    expect(within(dialog).queryByLabelText('Server URL')).toBeNull();

    fireEvent.click(within(dialog).getByRole('radio', { name: 'Remote server (HTTP)' }));
    expect(within(dialog).queryByLabelText('Program')).toBeNull();
    expect(within(dialog).getByLabelText('Server URL')).toBeTruthy();
    expect(within(dialog).queryByLabelText('Token')).toBeNull();
    fireEvent.change(within(dialog).getByLabelText('Authentication'), { target: { value: 'bearer' } });
    expect(within(dialog).getByLabelText('Token')).toBeTruthy();

    fireEvent.click(within(dialog).getByRole('radio', { name: 'Local program' }));
    fireEvent.change(within(dialog).getByLabelText('Name'), { target: { value: 'Notes' } });
    fireEvent.change(within(dialog).getByLabelText('Program'), { target: { value: 'node' } });
    fireEvent.change(within(dialog).getByLabelText('Arguments'), {
      target: { value: '/opt/notes server.js\n--read only\n' },
    });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Add server' }));
    await waitFor(() =>
      expect(mocks.saveMcpServer).toHaveBeenCalledWith(
        null,
        expect.objectContaining({
          name: 'Notes',
          transport: 'stdio',
          command: 'node',
          // One argument per line, never split on spaces.
          args: ['/opt/notes server.js', '--read only'],
        }),
      ),
    );
  });
});
