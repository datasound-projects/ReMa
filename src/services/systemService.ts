import { commands, type AppStatus } from '../generated/bindings';
import { callBackend } from './ipc';

export type { AppStatus, BackendStatus } from '../generated/bindings';

export function getAppStatus(): Promise<AppStatus> {
  return callBackend(() => commands.getAppStatus());
}

export function getSystemTimezone(): Promise<string> {
  return callBackend(() => commands.getSystemTimezone());
}

/** Opens an http(s)/mailto link in the default browser. */
export function openExternalUrl(url: string): Promise<null> {
  return callBackend(() => commands.openExternalUrl(url));
}
