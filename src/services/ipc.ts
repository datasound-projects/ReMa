import { invoke, isTauri, type InvokeArgs } from '@tauri-apps/api/core';

import type {
  ApiErrorCode,
  ApiErrorPayload,
  CommandArgs,
  CommandName,
  CommandResult,
} from '../types/api';

/** Normalized error for every failed backend call. */
export class ApiError extends Error {
  readonly code: ApiErrorCode;

  constructor(code: ApiErrorCode, message: string) {
    super(message);
    this.name = 'ApiError';
    this.code = code;
  }
}

function isApiErrorPayload(value: unknown): value is ApiErrorPayload {
  return (
    typeof value === 'object' &&
    value !== null &&
    typeof (value as ApiErrorPayload).code === 'string' &&
    typeof (value as ApiErrorPayload).message === 'string'
  );
}

/** Converts anything thrown by a backend call into an `ApiError`. */
export function toApiError(error: unknown): ApiError {
  if (error instanceof ApiError) return error;
  if (isApiErrorPayload(error)) return new ApiError(error.code, error.message);
  if (typeof error === 'string') return new ApiError('unknown', error);
  if (error instanceof Error) return new ApiError('unknown', error.message);
  return new ApiError('unknown', 'Unexpected backend error');
}

/**
 * Typed wrapper around Tauri's `invoke`. The only place in the frontend that
 * talks to the Rust backend directly; feature services build on top of it.
 */
export async function invokeCommand<C extends CommandName>(
  command: C,
  ...args: CommandArgs<C> extends undefined ? [] : [CommandArgs<C>]
): Promise<CommandResult<C>> {
  if (!isTauri()) {
    throw new ApiError(
      'backend_unavailable',
      'The ReMa backend is only available inside the desktop app (run: pnpm tauri dev).',
    );
  }

  try {
    return await invoke<CommandResult<C>>(command, args[0] as InvokeArgs | undefined);
  } catch (error) {
    throw toApiError(error);
  }
}
