import { useState } from 'react';

import { toApiError } from '../services/ipc';

/** Runs an action, tracking busy state and a readable error. */
export function useAction() {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const run = async (action: () => Promise<unknown>): Promise<boolean> => {
    setBusy(true);
    setError(null);
    try {
      await action();
      return true;
    } catch (err) {
      setError(toApiError(err).message);
      return false;
    } finally {
      setBusy(false);
    }
  };
  return { busy, error, run, clearError: () => setError(null) };
}
