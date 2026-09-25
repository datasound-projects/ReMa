import type { AppStatus } from '../types/system';
import { invokeCommand } from './ipc';

export function getAppStatus(): Promise<AppStatus> {
  return invokeCommand('get_app_status');
}
