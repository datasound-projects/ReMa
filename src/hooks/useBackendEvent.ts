import { useEffect, useRef } from 'react';

import { subscribe, type Listenable } from '../services/events';

/** Calls `handler` whenever the backend emits `event`. */
export function useBackendEvent<T>(event: Listenable<T>, handler: (payload: T) => void) {
  const handlerRef = useRef(handler);
  useEffect(() => {
    handlerRef.current = handler;
  });
  useEffect(() => subscribe(event, (payload) => handlerRef.current(payload)), [event]);
}
