/**
 * What a chat has selected in the composer's + menu: agents (their
 * instructions are added, in selection order), MCP servers (their tools
 * are offered to the model) and connectors (the user's mail, calendars and
 * applications; the model decides when to use one that is on, as in
 * Claude and ChatGPT). Independent of the Profile switch.
 */
import type { Agent } from '../services/agentService';
import type { ChatConnector } from '../services/chatService';
import type { ConnectorsOverview, ConnectorState } from '../services/connectorService';
import type { McpServer } from '../services/mcpService';

export interface ChatSelection {
  agentIds: string[];
  mcpServerIds: number[];
  /** Connectors on in this chat; `null`: every connected one (the default). */
  connectors: ChatConnector[] | null;
}

export const EMPTY_SELECTION: ChatSelection = { agentIds: [], mcpServerIds: [], connectors: null };

/** A connector the chat can turn on or off. */
export interface ConnectorOption {
  id: ChatConnector;
  name: string;
  /** Signed in and usable now; otherwise it needs attention in Settings. */
  ready: boolean;
  state: ConnectorState | null;
}

const CHAT_CONNECTORS: readonly ChatConnector[] = ['gmail', 'google_calendar', 'outlook_mail', 'outlook_calendar'];

/**
 * The connectors a chat can use: the mail and calendar connectors the user
 * added in Settings, and the application tracker (always there).
 */
export function connectorOptions(overview: ConnectorsOverview | null): ConnectorOption[] {
  const added = (overview?.connectors ?? []).flatMap((c): ConnectorOption[] => {
    const id = CHAT_CONNECTORS.find((x) => x === c.id);
    if (!id || !c.enabled || c.state === 'disconnected' || c.state === 'unavailable') return [];
    return [{ id, name: c.name, ready: c.state === 'connected' || c.state === 'syncing', state: c.state }];
  });
  return [...added, { id: 'applications', name: 'Applications', ready: true, state: null }];
}

export function connectorOn(selection: ChatSelection, id: ChatConnector): boolean {
  return selection.connectors === null || selection.connectors.includes(id);
}

/**
 * Turns one connector on or off. The first change turns "every connected
 * one" into an explicit list, so connectors added later stay off here.
 */
export function toggleConnector(
  selection: ChatSelection,
  id: ChatConnector,
  options: readonly ConnectorOption[],
): ChatSelection {
  const current = selection.connectors ?? options.map((o) => o.id);
  return { ...selection, connectors: toggle(current, id) };
}

/** Most agents one request can use (as in Rust). */
export const MAX_AGENTS = 8;

/** Adds an item at the end (selection order), or removes it. */
export function toggle<T>(list: readonly T[], item: T): T[] {
  return list.includes(item) ? list.filter((x) => x !== item) : [...list, item];
}

/** Only servers turned on in Settings can be picked in Chat. */
export function pickableServers(servers: readonly McpServer[]): McpServer[] {
  return servers.filter((s) => s.enabled);
}

export interface Reconciled {
  selection: ChatSelection;
  /** Selected servers that were turned off or removed in Settings. */
  removedServers: string[];
  changed: boolean;
}

/**
 * Drops what can no longer be used: agents that were deleted and servers
 * that were turned off or removed. Lists that are still loading (`null`)
 * leave their part untouched.
 */
export function reconcile(
  selection: ChatSelection,
  agents: readonly Agent[] | null,
  servers: readonly McpServer[] | null,
  knownNames: ReadonlyMap<number, string> = new Map(),
): Reconciled {
  const agentIds = agents ? selection.agentIds.filter((id) => agents.some((a) => a.id === id)) : selection.agentIds;
  let mcpServerIds = selection.mcpServerIds;
  const removedServers: string[] = [];
  if (servers) {
    mcpServerIds = selection.mcpServerIds.filter((id) => {
      const server = servers.find((s) => s.id === id);
      if (server?.enabled) return true;
      removedServers.push(server?.name ?? knownNames.get(id) ?? 'An MCP server');
      return false;
    });
  }
  const changed = agentIds.length !== selection.agentIds.length || mcpServerIds.length !== selection.mcpServerIds.length;
  return { selection: changed ? { ...selection, agentIds, mcpServerIds } : selection, removedServers, changed };
}

export function sameSelection(a: ChatSelection, b: ChatSelection): boolean {
  return (
    a.agentIds.length === b.agentIds.length &&
    a.agentIds.every((id, i) => b.agentIds[i] === id) &&
    a.mcpServerIds.length === b.mcpServerIds.length &&
    a.mcpServerIds.every((id, i) => b.mcpServerIds[i] === id) &&
    (a.connectors === null
      ? b.connectors === null
      : b.connectors !== null &&
        a.connectors.length === b.connectors.length &&
        a.connectors.every((id, i) => b.connectors?.[i] === id))
  );
}
