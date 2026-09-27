import { useCallback, useEffect, useRef, useState } from 'react';

import { backendEvents } from '../services/events';
import { toApiError } from '../services/ipc';
import { getTaskRun, listTaskRuns, type TaskRun, type TaskRunSummary } from '../services/taskService';
import { useBackendEvent } from './useBackendEvent';

type Status = 'loading' | 'ready' | 'error';

/**
 * Coalesces reloads: while one is in flight, further requests run it once
 * more afterwards (a run can report several changes a second).
 */
function useSerialLoader(load: () => Promise<void>) {
  const busy = useRef(false);
  const again = useRef(false);
  const loadRef = useRef(load);
  useEffect(() => {
    loadRef.current = load;
  });
  return useCallback(async () => {
    if (busy.current) {
      again.current = true;
      return;
    }
    busy.current = true;
    try {
      do {
        again.current = false;
        await loadRef.current();
      } while (again.current);
    } finally {
      busy.current = false;
    }
  }, []);
}

interface RunsState {
  taskId: number;
  runs: TaskRunSummary[];
  hasMore: boolean;
  status: Status;
  error: string | null;
}

const initial = (taskId: number): RunsState => ({
  taskId,
  runs: [],
  hasMore: false,
  status: 'loading',
  error: null,
});

/**
 * A task's run history, newest first, loaded a page at a time. The newest
 * page is reloaded whenever one of the task's runs changes; older pages
 * stay as loaded.
 */
export function useTaskRuns(taskId: number) {
  const [state, setState] = useState<RunsState>(() => initial(taskId));
  const [loadingMore, setLoadingMore] = useState(false);
  const current = useRef(taskId);
  // What is shown, for merging a reloaded first page with older pages.
  const shown = useRef<RunsState>(state);

  const show = useCallback((next: RunsState) => {
    shown.current = next;
    setState(next);
  }, []);

  const loadNewest = useCallback(async () => {
    const id = taskId;
    const before = shown.current.taskId === id ? shown.current : initial(id);
    try {
      const page = await listTaskRuns(id, null);
      if (current.current !== id) return;
      const oldest = page.runs.at(-1)?.id;
      const older = oldest === undefined ? [] : before.runs.filter((r) => r.id < oldest);
      show({
        taskId: id,
        runs: [...page.runs, ...older],
        hasMore: older.length === 0 ? page.hasMore : before.hasMore,
        status: 'ready',
        error: null,
      });
    } catch (err) {
      if (current.current !== id) return;
      const error = toApiError(err).message;
      show(before.status === 'ready' ? { ...before, error } : { ...initial(id), status: 'error', error });
    }
  }, [taskId, show]);
  const refresh = useSerialLoader(loadNewest);

  useEffect(() => {
    current.current = taskId;
    void refresh();
  }, [taskId, refresh]);

  useBackendEvent(backendEvents.taskRunChanged, (change) => {
    if (change.taskId === taskId) void refresh();
  });

  const view = state.taskId === taskId ? state : initial(taskId);

  const loadMore = useCallback(async () => {
    const last = shown.current.taskId === taskId ? shown.current.runs.at(-1) : undefined;
    if (!last || loadingMore) return;
    setLoadingMore(true);
    try {
      const page = await listTaskRuns(taskId, last.id);
      if (current.current !== taskId) return;
      show({
        ...shown.current,
        runs: [...shown.current.runs, ...page.runs.filter((r) => r.id < last.id)],
        hasMore: page.hasMore,
      });
    } catch (err) {
      show({ ...shown.current, error: toApiError(err).message });
    } finally {
      setLoadingMore(false);
    }
  }, [taskId, loadingMore, show]);

  return {
    runs: view.runs,
    hasMore: view.hasMore,
    status: view.status,
    error: view.error,
    loadingMore,
    loadMore,
    retry: refresh,
  };
}

interface RunState {
  runId: number | null;
  run: TaskRun | null;
  error: string | null;
}

/** One run with everything it recorded, kept up to date while it runs. */
export function useTaskRun(runId: number | null) {
  const [state, setState] = useState<RunState>({ runId, run: null, error: null });
  const current = useRef(runId);

  const load = useCallback(async () => {
    const id = runId;
    if (id === null) return;
    try {
      const run = await getTaskRun(id);
      if (current.current === id) setState({ runId: id, run, error: null });
    } catch (err) {
      if (current.current === id) {
        const error = toApiError(err).message;
        setState((s) => ({ runId: id, run: s.runId === id ? s.run : null, error }));
      }
    }
  }, [runId]);
  const refresh = useSerialLoader(load);

  useEffect(() => {
    current.current = runId;
    void refresh();
  }, [runId, refresh]);

  useBackendEvent(backendEvents.taskRunChanged, (change) => {
    if (change.runId === runId) void refresh();
  });

  return state.runId === runId ? { run: state.run, error: state.error } : { run: null, error: null };
}
