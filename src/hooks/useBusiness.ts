import { useCallback, useRef, useState } from 'react';

import { cancelBusinessRun, getBusinessOverview, getPipeline, requestKey } from '../services/businessService';
import { backendEvents } from '../services/events';
import { toApiError } from '../services/ipc';
import { useAsyncData } from './useAsyncData';
import { useBackendEvent } from './useBackendEvent';

/** Business Profile, offers, plans, experiments and drafts, kept current. */
export function useBusinessOverview() {
  const data = useAsyncData(getBusinessOverview);
  useBackendEvent(backendEvents.businessChanged, data.refresh);
  return data;
}

/** The commercial pipeline, kept current. */
export function usePipeline() {
  const data = useAsyncData(getPipeline);
  useBackendEvent(backendEvents.businessChanged, data.refresh);
  return data;
}

/**
 * One research request at a time: its run id, the backend's status lines
 * for it, stopping it, and its error. The status is what the backend
 * reports — never a made-up percentage.
 */
export function useBusinessRun() {
  const runRef = useRef<string | null>(null);
  const [running, setRunning] = useState(false);
  const [status, setStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useBackendEvent(backendEvents.businessProgress, (event) => {
    if (event.runId === runRef.current) setStatus(event.text);
  });

  const start = useCallback(async <T,>(work: (runId: string) => Promise<T>): Promise<T | null> => {
    if (runRef.current) return null;
    const runId = requestKey('run');
    runRef.current = runId;
    setRunning(true);
    setStatus(null);
    setError(null);
    try {
      return await work(runId);
    } catch (err) {
      setError(toApiError(err).message);
      return null;
    } finally {
      runRef.current = null;
      setRunning(false);
      setStatus(null);
    }
  }, []);

  const stop = useCallback(() => {
    const runId = runRef.current;
    if (runId) void cancelBusinessRun(runId).catch(() => undefined);
  }, []);

  return { running, status, error, start, stop, clearError: () => setError(null) };
}
