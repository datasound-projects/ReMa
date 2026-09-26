/**
 * What a chat has selected in the composer's + menu: agents (their
 * instructions are added, in selection order) and MCP servers (their tools
 * are offered to the model). Independent of the Profile switch.
 */
import type { Agent } from '../services/agentService';
import type { McpServer } from '../services/mcpService';

export interface ChatSelection {
  agentIds: string[];
  mcpServerIds: number[];
}

export const EMPTY_SELECTION: ChatSelection = { agentIds: [], mcpServerIds: [] };

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
  return { selection: changed ? { agentIds, mcpServerIds } : selection, removedServers, changed };
}

export function sameSelection(a: ChatSelection, b: ChatSelection): boolean {
  return (
    a.agentIds.length === b.agentIds.length &&
    a.agentIds.every((id, i) => b.agentIds[i] === id) &&
    a.mcpServerIds.length === b.mcpServerIds.length &&
    a.mcpServerIds.every((id, i) => b.mcpServerIds[i] === id)
  );
}
