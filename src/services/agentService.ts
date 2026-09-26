import { commands, type Agent, type AgentInput } from '../generated/bindings';
import { callBackend } from './ipc';

export type { Agent, AgentInput } from '../generated/bindings';

/** Built-in agents first, then the user's own. */
export function listAgents(): Promise<Agent[]> {
  return callBackend(() => commands.listAgents());
}

/** Creates (`id` = null) or updates a custom agent (`id` is its number). */
export function saveAgent(id: number | null, input: AgentInput): Promise<Agent> {
  return callBackend(() => commands.saveAgent(id, input));
}

/** Copies an agent (built-in or custom) into a new custom agent. */
export function duplicateAgent(agentId: string): Promise<Agent> {
  return callBackend(() => commands.duplicateAgent(agentId));
}

export function deleteAgent(id: number): Promise<null> {
  return callBackend(() => commands.deleteAgent(id));
}

/** The number of a custom agent (`custom:<n>`), or `null` for built-ins. */
export function customAgentNumber(agent: Pick<Agent, 'id'>): number | null {
  const match = /^custom:(\d+)$/.exec(agent.id);
  return match ? Number(match[1]) : null;
}
