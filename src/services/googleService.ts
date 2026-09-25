import { commands, type GoogleService, type GoogleStatus } from '../generated/bindings';
import { callBackend } from './ipc';

export type { GoogleService, GoogleStatus } from '../generated/bindings';

/** Connection state only; tokens never leave the Rust core. */
export function getGoogleStatus(): Promise<GoogleStatus> {
  return callBackend(() => commands.getGoogleStatus());
}

export function saveGoogleClient(clientId: string, clientSecret: string): Promise<GoogleStatus> {
  return callBackend(() => commands.saveGoogleClient(clientId, clientSecret));
}

/** Opens Google sign-in in the browser; resolves when the user has finished. */
export function connectGoogle(): Promise<GoogleStatus> {
  return callBackend(() => commands.connectGoogle());
}

export function cancelGoogleConnect(): Promise<null> {
  return callBackend(() => commands.cancelGoogleConnect());
}

export function disconnectGoogle(): Promise<GoogleStatus> {
  return callBackend(() => commands.disconnectGoogle());
}

export function setGoogleServiceEnabled(service: GoogleService, enabled: boolean): Promise<GoogleStatus> {
  return callBackend(() => commands.setGoogleServiceEnabled(service, enabled));
}
