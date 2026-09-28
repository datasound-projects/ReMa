import {
  commands,
  type McpImportCandidate,
  type McpImportRequest,
  type McpImportResult,
  type McpImportSource,
  type McpServer,
  type McpServerInput,
  type McpTestResult,
} from '../generated/bindings';
import { callBackend } from './ipc';

export type {
  McpAuth,
  McpImportApp,
  McpImportCandidate,
  McpImportRequest,
  McpImportResult,
  McpImportSource,
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

/**
 * MCP servers configured in other apps on this computer (Claude Desktop,
 * Claude Code, Cursor, VS Code, Windsurf). Names and programs only.
 */
export function mcpImportSources(): Promise<McpImportSource[]> {
  return callBackend(() => commands.mcpImportSources());
}

/** The servers in pasted JSON (an `mcpServers` block or one server). */
export function previewMcpImport(json: string): Promise<McpImportCandidate[]> {
  return callBackend(() => commands.previewMcpImport(json));
}

/** Adds the chosen servers; values go straight to the keychain. */
export function importMcpServers(request: McpImportRequest): Promise<McpImportResult> {
  return callBackend(() => commands.importMcpServers(request));
}
