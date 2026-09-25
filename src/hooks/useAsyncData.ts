import { useCallback, useEffect, useState } from 'react';

import { toApiError, type ApiError } from '../services/ipc';
import type { AsyncState } from '../types/async';

/**
 * Loads data from a frontend service and tracks loading / success / error.
 * `retry` runs the load again.
 *
 * `load` must be stable between renders (a module-level service function,
 * or wrapped in `useCallback`), otherwise it re-runs on every render.
 */
export function useAsyncData<T>(load: () => Promise<T>) {
  const [state, setState] = useState<AsyncState<T, ApiError>>({ status: 'loading' });
  const [attempt, setAttempt] = useState(0);

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
  }, [load, attempt]);

  const retry = useCallback(() => {
    setState({ status: 'loading' });
    setAttempt((n) => n + 1);
  }, []);

  return { state, retry };
}
