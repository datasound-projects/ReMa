import { useCallback, useEffect, useRef, useState } from 'react';

import { backendEvents } from '../services/events';
import { toApiError } from '../services/ipc';
import { useBackendEvent } from './useBackendEvent';

export interface LiveData<T> {
  /** The latest result; kept while a new one loads (no flashing). */
  data: T | null;
  error: string | null;
  loading: boolean;
  refresh: () => void;
}

/**
 * Loads an analytics view whenever `key` (the serialized request) changes,
 * and again when job data or the Profile change in the background. Requests
 * are debounced so typing in a filter does not flood the backend; results of
 * outdated requests are ignored.
 */
export function useAnalyticsData<T>(load: (() => Promise<T>) | null, key: string): LiveData<T> {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [version, setVersion] = useState(0);
  const loadRef = useRef(load);
  const request = useRef(0);
  const enabled = load !== null;

  useEffect(() => {
    loadRef.current = load;
  });

  useEffect(() => {
    if (!loadRef.current) return;
    const id = ++request.current;
    const timer = window.setTimeout(() => {
      const run = loadRef.current;
      if (!run) return;
      setLoading(true);
      run()
        .then((result) => {
          if (id !== request.current) return;
          setData(result);
          setError(null);
        })
        .catch((err: unknown) => {
          if (id === request.current) setError(toApiError(err).message);
        })
        .finally(() => {
          if (id === request.current) setLoading(false);
        });
    }, 180);
    return () => window.clearTimeout(timer);
  }, [key, version, enabled]);

  // Background changes (new searches, job details, research, Profile edits).
  const pending = useRef<number | null>(null);
  const soon = useCallback(() => {
    if (pending.current != null) window.clearTimeout(pending.current);
    pending.current = window.setTimeout(() => setVersion((v) => v + 1), 400);
  }, []);
  useEffect(() => () => {
    if (pending.current != null) window.clearTimeout(pending.current);
  }, []);
  useBackendEvent(backendEvents.analyticsChanged, soon);
  useBackendEvent(backendEvents.profileChanged, soon);

  const refresh = useCallback(() => setVersion((v) => v + 1), []);
  return { data, error, loading, refresh };
}
