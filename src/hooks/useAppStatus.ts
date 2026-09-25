import { useCallback, useEffect, useState } from 'react';

import { toApiError, type ApiError } from '../services/ipc';
import { getAppStatus } from '../services/systemService';
import type { AsyncState } from '../types/async';
import type { AppStatus } from '../types/system';

/** Loads the backend status on mount; `refresh` re-runs the check. */
export function useAppStatus() {
  const [state, setState] = useState<AsyncState<AppStatus, ApiError>>({ status: 'loading' });
  const [requestId, setRequestId] = useState(0);

  useEffect(() => {
    let active = true;

    getAppStatus()
      .then((data) => {
        if (active) setState({ status: 'success', data });
      })
      .catch((error: unknown) => {
        if (active) setState({ status: 'error', error: toApiError(error) });
      });

    return () => {
      active = false;
    };
  }, [requestId]);

  const refresh = useCallback(() => {
    setState({ status: 'loading' });
    setRequestId((id) => id + 1);
  }, []);

  return { state, refresh };
}
