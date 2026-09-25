import { useCallback, useEffect, useState } from 'react';

import { toApiError, type ApiError } from '../services/ipc';
import type { AsyncState } from '../types/async';

/**
 * Loads data from a frontend service and tracks loading / success / error.
 *
 * - `retry` shows the loading state again and reloads (after an error).
 * - `refresh` reloads in the background, keeping the current data visible
 *   (used when the backend reports a change).
 *
 * `load` must be stable between renders (a module-level service function,
 * or wrapped in `useCallback`), otherwise it re-runs on every render.
 */
export function useAsyncData<T>(load: () => Promise<T>) {
  const [state, setState] = useState<AsyncState<T, ApiError>>({ status: 'loading' });
  const [version, setVersion] = useState(0);

  useEffect(() => {
    let active = true;

    load()
      .then((data) => {
        if (active) setState({ status: 'success', data });
      })
      .catch((error: unknown) => {
        if (active) setState({ status: 'error', error: toApiError(error) });
      });

    return () => {
      active = false;
    };
  }, [load, version]);

  const retry = useCallback(() => {
    setState({ status: 'loading' });
    setVersion((n) => n + 1);
  }, []);

  const refresh = useCallback(() => setVersion((n) => n + 1), []);

  return { state, retry, refresh };
}

/** The loaded data, or `fallback` while loading or on error. */
export function dataOr<T, F>(state: AsyncState<T, ApiError>, fallback: F): T | F {
  return state.status === 'success' ? state.data : fallback;
}
