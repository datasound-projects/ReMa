import { commands, type RemaMcpStatus } from '../generated/bindings';
import { callBackend } from './ipc';

export type { Readiness, RemaMcpStatus, SourceSummary } from '../generated/bindings';

/** ReMa MCP (built in): on/off, readiness and sources. `check` also lists its tools over MCP. */
export function remaMcpStatus(check = false): Promise<RemaMcpStatus> {
  return callBackend(() => commands.remaMcpStatus(check));
}

/** Only called after the two confirmations (turning off) or with one click (turning on). */
export function setRemaMcpEnabled(enabled: boolean): Promise<RemaMcpStatus> {
  return callBackend(() => commands.setRemaMcpEnabled(enabled));
}

/** Clears ReMa MCP's cached jobs and searches (never chats, Analytics or Profile). */
export function clearRemaMcpCache(): Promise<RemaMcpStatus> {
  return callBackend(() => commands.clearRemaMcpCache());
}
