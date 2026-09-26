import { commands, type Appearance, type AppStatus } from '../generated/bindings';
import { callBackend } from './ipc';

export type { Appearance, AppStatus, BackendStatus } from '../generated/bindings';

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

/** Remembers the theme and applies it to the window (native menus, next launch). */
export function setAppearance(appearance: Appearance): Promise<null> {
  return callBackend(() => commands.setAppearance(appearance));
}
