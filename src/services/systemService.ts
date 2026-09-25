import { commands, type AppStatus } from '../generated/bindings';
import { callBackend } from './ipc';

export type { AppStatus, BackendStatus } from '../generated/bindings';

export function getAppStatus(): Promise<AppStatus> {
  return callBackend(() => commands.getAppStatus());
}
