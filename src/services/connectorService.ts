import {
  commands,
  type ConnectorId,
  type ConnectorPreferences,
  type ConnectorsOverview,
  type ProviderId,
} from '../generated/bindings';
import { callBackend } from './ipc';

export type {
  BackgroundSettings,
  Capability,
  ConnectorId,
  ConnectorKind,
  ConnectorPreferences,
  ConnectorState,
  ConnectorStatus,
  ConnectorsOverview,
  InterviewMode,
  PermissionView,
  ProviderId,
} from '../generated/bindings';

/** Every connector's state. Tokens never leave the Rust core. */
export function getConnectors(): Promise<ConnectorsOverview> {
  return callBackend(() => commands.getConnectors());
}

/**
 * Opens the provider's sign-in in the default browser; resolves once the
 * user has finished there (or rejects if the sign-in failed or was cancelled).
 */
export function connectConnector(id: ConnectorId): Promise<ConnectorsOverview> {
  return callBackend(() => commands.connectConnector(id));
}

export function cancelConnectorSignIn(provider: ProviderId): Promise<null> {
  return callBackend(() => commands.cancelConnectorSignIn(provider));
}

/** Stops syncing; the account's last connector also signs out. History is kept. */
export function disconnectConnector(id: ConnectorId): Promise<ConnectorsOverview> {
  return callBackend(() => commands.disconnectConnector(id));
}

export function syncConnector(id: ConnectorId): Promise<ConnectorsOverview> {
  return callBackend(() => commands.syncConnector(id));
}

export function setConnectorBackgroundSync(id: ConnectorId, enabled: boolean): Promise<ConnectorsOverview> {
  return callBackend(() => commands.setConnectorBackgroundSync(id, enabled));
}

export function setConnectorPreferences(preferences: ConnectorPreferences): Promise<ConnectorsOverview> {
  return callBackend(() => commands.setConnectorPreferences(preferences));
}

export function setBackgroundSettings(runInBackground: boolean, startAtLogin: boolean): Promise<ConnectorsOverview> {
  return callBackend(() => commands.setBackgroundSettings(runInBackground, startAtLogin));
}
