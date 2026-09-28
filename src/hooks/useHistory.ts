import { useCallback, useRef, useState } from 'react';

interface HistoryState<T> {
  present: T;
  past: T[];
  future: T[];
}

const LIMIT = 200;
/** Edits closer together than this (typing) become one undo step. */
const COALESCE_MS = 700;

/**
 * Undo/redo over a value. Quick successive edits with the same `group`
 * (for example typing in one field) are one step.
 */
export function useHistory<T>(initial: T) {
  const [state, setState] = useState<HistoryState<T>>({ present: initial, past: [], future: [] });
  const last = useRef<{ group: string | null; at: number }>({ group: null, at: 0 });

  const set = useCallback((update: (value: T) => T, group: string | null = null) => {
    const now = Date.now();
    const coalesce = group !== null && group === last.current.group && now - last.current.at < COALESCE_MS;
    last.current = { group, at: now };
    setState((s) => {
      const next = update(s.present);
      if (next === s.present) return s;
      const past = coalesce ? s.past : [...s.past, s.present].slice(-LIMIT);
      return { present: next, past, future: [] };
    });
  }, []);

  /** Replaces the value without an undo step (loading, recovery). */
  const reset = useCallback((value: T) => {
    last.current = { group: null, at: 0 };
    setState({ present: value, past: [], future: [] });
  }, []);

  const undo = useCallback(() => {
    last.current = { group: null, at: 0 };
    setState((s) => {
      const previous = s.past[s.past.length - 1];
      if (previous === undefined) return s;
      return { present: previous, past: s.past.slice(0, -1), future: [s.present, ...s.future] };
    });
  }, []);

  const redo = useCallback(() => {
    last.current = { group: null, at: 0 };
    setState((s) => {
      const [next, ...rest] = s.future;
      if (next === undefined) return s;
      return { present: next, past: [...s.past, s.present], future: rest };
    });
  }, []);

  return { value: state.present, set, reset, undo, redo, canUndo: state.past.length > 0, canRedo: state.future.length > 0 };
}
