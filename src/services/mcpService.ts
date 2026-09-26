import { commands, type McpServer, type McpServerInput, type McpTestResult } from '../generated/bindings';
import { callBackend } from './ipc';

export type {
  McpAuth,
  McpEnvVar,
  McpServer,
  McpServerInput,
  McpState,
  McpStatus,
  McpTestResult,
  McpToolInfo,
  McpTransport,
} from '../generated/bindings';

/** Configured servers. Secret values never reach the interface. */
export function listMcpServers(): Promise<McpServer[]> {
  return callBackend(() => commands.listMcpServers());
}

/** Adds (`id` = null; new servers start turned off) or edits a server. */
export function saveMcpServer(id: number | null, input: McpServerInput): Promise<McpServer> {
  return callBackend(() => commands.saveMcpServer(id, input));
}

export function deleteMcpServer(id: number): Promise<null> {
  return callBackend(() => commands.deleteMcpServer(id));
}

export function setMcpServerEnabled(id: number, enabled: boolean): Promise<McpServer> {
  return callBackend(() => commands.setMcpServerEnabled(id, enabled));
}

/** Connects, or reconnects, an enabled server. */
export function connectMcpServer(id: number): Promise<McpServer> {
  return callBackend(() => commands.connectMcpServer(id));
}

export function disconnectMcpServer(id: number): Promise<McpServer> {
  return callBackend(() => commands.disconnectMcpServer(id));
}

/** Tries a configuration (the form's current values) without saving it. */
export function testMcpServer(id: number | null, input: McpServerInput): Promise<McpTestResult> {
  return callBackend(() => commands.testMcpServer(id, input));
}

/** Signs in with OAuth in the system browser; resolves when done. */
export function signInMcpServer(id: number): Promise<McpServer> {
  return callBackend(() => commands.signInMcpServer(id));
}

export function cancelMcpSignIn(id: number): Promise<McpServer> {
  return callBackend(() => commands.cancelMcpSignIn(id));
}

export function signOutMcpServer(id: number): Promise<McpServer> {
  return callBackend(() => commands.signOutMcpServer(id));
}
