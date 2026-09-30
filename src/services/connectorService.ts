import {
  commands,
  type AppRegistrationInput,
  type ConnectionPreferences,
  type ConnectorId,
  type ConnectorsOverview,
  type ProviderId,
} from '../generated/bindings';
import { callBackend } from './ipc';

export type {
  AppRegistration,
  AppRegistrationInput,
  BackgroundSettings,
  Capability,
  CapabilityView,
  ConnectionPreferences,
  ConnectionState,
  ConnectorErrorCode,
  ConnectorId,
  ConnectorKind,
  ConnectorState,
  ConnectorStatus,
  ConnectorsOverview,
  MailProcessing,
  PermissionView,
  ProviderAccount,
  ProviderId,
  RegistrationSource,
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

/**
 * Connects a provider account with every capability ReMa offers for it
 * (Google: Gmail and Google Calendar) in one browser sign-in.
 */
export function connectProviderAccount(provider: ProviderId): Promise<ConnectorsOverview> {
  return callBackend(() => commands.connectProviderAccount(provider));
}

/** Signs the account out entirely: revokes where possible and deletes the stored sign-in. */
export function disconnectProviderAccount(provider: ProviderId): Promise<ConnectorsOverview> {
  return callBackend(() => commands.disconnectProviderAccount(provider));
}

/** How connections meet chats. */
export function setConnectionPreferences(preferences: ConnectionPreferences): Promise<ConnectorsOverview> {
  return callBackend(() => commands.setConnectionPreferences(preferences));
}

export function cancelConnectorSignIn(provider: ProviderId): Promise<null> {
  return callBackend(() => commands.cancelConnectorSignIn(provider));
}

/** Stops syncing; the account's last connector also signs out. History is kept. */
export function disconnectConnector(id: ConnectorId): Promise<ConnectorsOverview> {
  return callBackend(() => commands.disconnectConnector(id));
}

export function setBackgroundSettings(runInBackground: boolean, startAtLogin: boolean): Promise<ConnectorsOverview> {
  return callBackend(() => commands.setBackgroundSettings(runInBackground, startAtLogin));
}

/**
 * Enters ReMa's app registration for a provider this copy was built without
 * (Settings → Connectors → Set up). Client IDs stay on this computer, a
 * Google client secret in the system keychain; the sign-in works right away.
 */
export function setAppRegistration(provider: ProviderId, input: AppRegistrationInput): Promise<ConnectorsOverview> {
  return callBackend(() => commands.setAppRegistration(provider, input));
}

/** Removes a registration entered in Settings (the account is disconnected first). */
export function removeAppRegistration(provider: ProviderId): Promise<ConnectorsOverview> {
  return callBackend(() => commands.removeAppRegistration(provider));
}
