import type { AppStatus } from './system';

/**
 * Every Tauri command the frontend may call, with its argument and result
 * types. Adding a command on the Rust side means adding an entry here.
 */
export interface CommandMap {
  get_app_status: { args: undefined; result: AppStatus };
}

export type CommandName = keyof CommandMap;
export type CommandArgs<C extends CommandName> = CommandMap[C]['args'];
export type CommandResult<C extends CommandName> = CommandMap[C]['result'];

/** Error codes produced by the Rust `AppError` plus frontend-side failures. */
export type ApiErrorCode = 'internal' | 'backend_unavailable' | 'unknown';

/** Shape of a serialized Rust `AppError`. */
export interface ApiErrorPayload {
  code: ApiErrorCode;
  message: string;
}
