import { isTauri } from '@tauri-apps/api/core';

import type { ErrorCode, ErrorPayload } from '../generated/bindings';

/** Backend error codes (generated from Rust) plus frontend-side failures. */
export type ApiErrorCode = ErrorCode | 'backend_unavailable' | 'unknown';

/** Normalized error for every failed backend call. */
export class ApiError extends Error {
  readonly code: ApiErrorCode;

  constructor(code: ApiErrorCode, message: string) {
    super(message);
    this.name = 'ApiError';
    this.code = code;
  }
}

function isErrorPayload(value: unknown): value is ErrorPayload {
  return (
    typeof value === 'object' &&
    value !== null &&
    typeof (value as ErrorPayload).code === 'string' &&
    typeof (value as ErrorPayload).message === 'string'
  );
}

/** Converts anything thrown by a backend call into an `ApiError`. */
export function toApiError(error: unknown): ApiError {
  if (error instanceof ApiError) return error;
  if (isErrorPayload(error)) return new ApiError(error.code, error.message);
  if (typeof error === 'string') return new ApiError('unknown', error);
  if (error instanceof Error) return new ApiError('unknown', error.message);
  return new ApiError('unknown', 'Unexpected backend error');
}

/**
 * Runs a generated command binding with consistent error handling.
 * Feature services wrap every backend call in this:
 *
 *   callBackend(() => commands.getAppStatus())
 */
export async function callBackend<T>(call: () => Promise<T>): Promise<T> {
  if (!isTauri()) {
    throw new ApiError(
      'backend_unavailable',
      'The ReMa backend is only available inside the desktop app (run: pnpm tauri dev).',
    );
  }

  try {
    return await call();
  } catch (error) {
    throw toApiError(error);
  }
}
