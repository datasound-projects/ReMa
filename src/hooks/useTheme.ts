import { useSyncExternalStore } from 'react';

import { getTheme, subscribeTheme, type Theme } from '../lib/theme';

/** The current color theme; re-renders when it changes. */
export function useTheme(): Theme {
  return useSyncExternalStore(subscribeTheme, getTheme);
}
