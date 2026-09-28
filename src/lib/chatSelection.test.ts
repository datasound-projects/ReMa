import { describe, expect, it } from 'vitest';

import type { Agent } from '../services/agentService';
import type { ConnectorStatus, ConnectorsOverview } from '../services/connectorService';
import type { McpServer } from '../services/mcpService';
import type { ToolActivity } from '../services/chatService';
import {
  EMPTY_SELECTION,
  connectorOn,
  connectorOptions,
  pickableServers,
  reconcile,
  sameSelection,
  toggle,
  toggleConnector,
} from './chatSelection';
import { withActivity } from './toolActivity';

const agent = (id: string): Agent => ({
  id,
  name: id,
  description: '',
  instructions: 'x',
  icon: 'spark',
  builtin: id.startsWith('builtin:'),
  updatedAt: null,
});

const server = (id: number, name: string, enabled: boolean): McpServer => ({
  id,
  name,
  transport: 'stdio',
  command: 'node',
  args: [],
  envNames: [],
  cwd: '',
  url: '',
  auth: 'none',
  headerName: '',
  hasSecret: false,
  enabled,
  status: { state: enabled ? 'disconnected' : 'disabled', message: null, tools: [], protocolVersion: null, serverInfo: null },
  createdAt: 0,
  updatedAt: 0,
});

const status = (
  id: ConnectorStatus['id'],
  name: string,
  state: ConnectorStatus['state'],
  enabled = true,
): ConnectorStatus => ({
  id,
  provider: id.startsWith('outlook') ? 'microsoft' : 'google',
  kind: id.includes('calendar') ? 'calendar' : 'mail',
  name,
  publisher: '',
  description: '',
  state,
  enabled,
  accountEmail: null,
  accountName: null,
  permissions: [],
  lastSyncStartedAt: null,
  lastSyncAt: null,
  message: null,
  detail: null,
  errorCode: null,
});

const overview = (connectors: ConnectorStatus[]): ConnectorsOverview =>
  ({ connectors, background: null, mailProcessing: null }) as unknown as ConnectorsOverview;

describe('chat selection', () => {
  it('keeps selection order and removes on a second toggle', () => {
    let agents: string[] = [];
    agents = toggle(agents, 'builtin:job-match');
    agents = toggle(agents, 'builtin:cv-tailoring');
    agents = toggle(agents, 'custom:3');
    expect(agents).toEqual(['builtin:job-match', 'builtin:cv-tailoring', 'custom:3']);
    agents = toggle(agents, 'builtin:cv-tailoring');
    expect(agents).toEqual(['builtin:job-match', 'custom:3']);
  });

  it('offers only servers turned on in Settings', () => {
    const servers = [server(1, 'LinkedIn MCP', true), server(2, 'Local Files', false), server(3, 'Notes', true)];
    expect(pickableServers(servers).map((s) => s.name)).toEqual(['LinkedIn MCP', 'Notes']);
  });

  it('drops servers turned off or removed, and deleted agents, with their names', () => {
    const agents = [agent('builtin:job-search'), agent('custom:1')];
    const servers = [server(1, 'LinkedIn MCP', true), server(2, 'Local Files', false)];
    const result = reconcile(
      { agentIds: ['custom:1', 'custom:9', 'builtin:job-search'], mcpServerIds: [2, 1, 7], connectors: ['gmail'] },
      agents,
      servers,
      new Map([[7, 'Old server']]),
    );
    expect(result.changed).toBe(true);
    expect(result.selection).toEqual({
      agentIds: ['custom:1', 'builtin:job-search'],
      mcpServerIds: [1],
      connectors: ['gmail'],
    });
    expect(result.removedServers).toEqual(['Local Files', 'Old server']);
  });

  it('leaves the selection alone while lists are loading', () => {
    const selection = { agentIds: ['custom:1'], mcpServerIds: [4], connectors: null };
    const result = reconcile(selection, null, null);
    expect(result.changed).toBe(false);
    expect(result.selection).toBe(selection);
    expect(sameSelection(selection, { agentIds: ['custom:1'], mcpServerIds: [4], connectors: null })).toBe(true);
    expect(sameSelection(selection, { agentIds: [], mcpServerIds: [4], connectors: null })).toBe(false);
    expect(sameSelection(selection, { ...selection, connectors: [] })).toBe(false);
    expect(sameSelection({ ...selection, connectors: ['gmail'] }, { ...selection, connectors: ['gmail'] })).toBe(true);
  });

  it('offers the connectors added in Settings and the application tracker', () => {
    const options = connectorOptions(overview([
      status('gmail', 'Gmail', 'connected'),
      status('google_calendar', 'Google Calendar', 'reauth_required'),
      status('outlook_mail', 'Outlook Mail', 'disconnected', false),
      status('outlook_calendar', 'Outlook Calendar', 'unavailable', false),
      status('linkedin', 'LinkedIn', 'connected'),
    ]));
    expect(options.map((o) => [o.id, o.ready])).toEqual([
      ['gmail', true],
      ['google_calendar', false],
      ['applications', true],
    ]);
    expect(connectorOptions(null).map((o) => o.id)).toEqual(['applications']);
  });

  it('has every connected connector on until the chat turns one off', () => {
    const options = connectorOptions(overview([
      status('gmail', 'Gmail', 'connected'),
      status('outlook_calendar', 'Outlook Calendar', 'syncing'),
    ]));
    expect(connectorOn(EMPTY_SELECTION, 'gmail')).toBe(true);

    const off = toggleConnector(EMPTY_SELECTION, 'gmail', options);
    expect(off.connectors).toEqual(['outlook_calendar', 'applications']);
    expect(connectorOn(off, 'gmail')).toBe(false);
    expect(connectorOn(off, 'applications')).toBe(true);

    const on = toggleConnector(off, 'gmail', options);
    expect(on.connectors).toEqual(['outlook_calendar', 'applications', 'gmail']);
  });

  it('updates a tool call in place as its status changes', () => {
    const call = (status: ToolActivity['status']): ToolActivity => ({
      id: 'call-1',
      serverId: 1,
      server: 'Notes',
      tool: 'add_note',
      status,
      arguments: '{}',
      detail: null,
      readOnly: false,
    });
    let list = withActivity([], call('awaiting_approval'));
    list = withActivity(list, { ...call('running'), id: 'call-2' });
    list = withActivity(list, call('completed'));
    expect(list.map((a) => [a.id, a.status])).toEqual([
      ['call-1', 'completed'],
      ['call-2', 'running'],
    ]);
  });
});
