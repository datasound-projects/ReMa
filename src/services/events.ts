import { isTauri } from '@tauri-apps/api/core';
import type { EventCallback, UnlistenFn } from '@tauri-apps/api/event';

import { events } from '../generated/bindings';

/** Typed backend events (generated from Rust). */
export const backendEvents = events;

export interface Listenable<T> {
  listen: (callback: EventCallback<T>) => Promise<UnlistenFn>;
}

/**
 * Unsubscribes. Tauri's unlisten is async underneath and rejects when the
 * listener is already gone (e.g. React's double effects in development).
 */
function release(unlisten: UnlistenFn) {
  void Promise.resolve((unlisten as () => unknown)()).catch(() => {});
}

/** Subscribes to a backend event; returns an unsubscribe function. */
export function subscribe<T>(event: Listenable<T>, handler: (payload: T) => void): () => void {
  if (!isTauri()) return () => {};
  let unlisten: UnlistenFn | null = null;
  let cancelled = false;
  void event.listen((e) => handler(e.payload)).then((fn) => {
    if (cancelled) release(fn);
    else unlisten = fn;
  });
  return () => {
    cancelled = true;
    if (unlisten) release(unlisten);
  };
}
