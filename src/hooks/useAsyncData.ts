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
    const started = performance.now();

    load()
      .then((data) => {
        noteSlowLoad(load, started);
        if (active) setState({ status: 'success', data });
      })
      .catch((error: unknown) => {
        noteSlowLoad(load, started);
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

/** A load that took longer than this is noted in the console (with its
 * name, never its data), so a slow Settings section can be traced to the
 * backend call behind it. */
export const SLOW_LOAD_MS = 2_000;

function noteSlowLoad(load: () => unknown, started: number) {
  const took = performance.now() - started;
  if (took > SLOW_LOAD_MS) {
    console.warn(`Slow load: ${load.name || 'anonymous'} took ${Math.round(took)} ms`);
  }
}

/** The loaded data, or `fallback` while loading or on error. */
export function dataOr<T, F>(state: AsyncState<T, ApiError>, fallback: F): T | F {
  return state.status === 'success' ? state.data : fallback;
}
