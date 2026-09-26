// @vitest-environment jsdom
import '../../test/dom';

import { fireEvent, render, screen } from '@testing-library/react';
import { useState } from 'react';
import { describe, expect, it, vi } from 'vitest';

import { NavigationContext, type View } from '../../app/navigation';
import { EMPTY_SELECTION, pickableServers, type ChatSelection } from '../../lib/chatSelection';
import type { Agent } from '../../services/agentService';
import type { McpServer } from '../../services/mcpService';
import { ChatToolsMenu, SelectionChips } from './ChatToolsMenu';

const agent = (id: string, name: string): Agent => ({
  id,
  name,
  description: '',
  instructions: 'x',
  icon: 'spark',
  builtin: id.startsWith('builtin:'),
  updatedAt: null,
});

const server = (id: number, name: string, enabled: boolean): McpServer => ({
  id,
  name,
  transport: 'http',
  command: '',
  args: [],
  envNames: [],
  cwd: '',
  url: 'https://example.com/mcp',
  auth: 'none',
  headerName: '',
  hasSecret: false,
  enabled,
  status: { state: enabled ? 'connected' : 'disabled', message: null, tools: [], protocolVersion: null, serverInfo: null },
  createdAt: 0,
  updatedAt: 0,
});

const AGENTS = [agent('builtin:job-match', 'Job Match Analyst'), agent('builtin:cv-tailoring', 'CV Tailoring Agent'), agent('custom:2', 'Cover Letters')];
const SERVERS = [server(1, 'LinkedIn MCP', true), server(2, 'Disabled MCP', false), server(3, 'Local Files MCP', true)];

function Harness({ onChange }: { onChange: (s: ChatSelection) => void }) {
  const [selection, setSelection] = useState<ChatSelection>(EMPTY_SELECTION);
  const change = (next: ChatSelection) => {
    setSelection(next);
    onChange(next);
  };
  return (
    <NavigationContext value={{ view: { page: 'chat', conversationId: null }, navigate: vi.fn<(v: View) => void>() }}>
      <SelectionChips agents={AGENTS} servers={SERVERS} selection={selection} onChange={change} />
      <ChatToolsMenu agents={AGENTS} servers={pickableServers(SERVERS)} selection={selection} onChange={change} />
    </NavigationContext>
  );
}

describe('Chat + menu', () => {
  it('lists agents and only MCP servers turned on in Settings', () => {
    render(<Harness onChange={() => {}} />);
    fireEvent.click(screen.getByRole('button', { name: 'Agents and MCP tools' }));
    const items = screen.getAllByRole('menuitemcheckbox').map((i) => i.textContent);
    expect(items).toEqual(['Job Match Analyst', 'CV Tailoring Agent', 'Cover Letters', 'LinkedIn MCPConnected', 'Local Files MCPConnected']);
    expect(screen.queryByText('Disabled MCP')).toBeNull();
  });

  it('selects several agents and servers, shown as removable chips in selection order', () => {
    const onChange = vi.fn<(s: ChatSelection) => void>();
    render(<Harness onChange={onChange} />);
    fireEvent.click(screen.getByRole('button', { name: 'Agents and MCP tools' }));
    fireEvent.click(screen.getByRole('menuitemcheckbox', { name: /CV Tailoring Agent/ }));
    fireEvent.click(screen.getByRole('menuitemcheckbox', { name: /Job Match Analyst/ }));
    fireEvent.click(screen.getByRole('menuitemcheckbox', { name: /Local Files MCP/ }));
    fireEvent.click(screen.getByRole('menuitemcheckbox', { name: /LinkedIn MCP/ }));
    expect(onChange).toHaveBeenLastCalledWith({
      agentIds: ['builtin:cv-tailoring', 'builtin:job-match'],
      mcpServerIds: [3, 1],
    });
    const chips = screen.getByLabelText('Selected agents and MCP servers');
    expect(chips.textContent).toContain('CV Tailoring Agent');
    expect(chips.textContent).toContain('LinkedIn MCP');

    fireEvent.click(screen.getByRole('button', { name: 'Remove CV Tailoring Agent' }));
    expect(onChange).toHaveBeenLastCalledWith({ agentIds: ['builtin:job-match'], mcpServerIds: [3, 1] });
    fireEvent.click(screen.getByRole('button', { name: 'Remove Local Files MCP' }));
    expect(onChange).toHaveBeenLastCalledWith({ agentIds: ['builtin:job-match'], mcpServerIds: [1] });
  });
});
