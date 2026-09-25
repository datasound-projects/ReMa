import { createContext, useContext } from 'react';

import type { AnalyticsPreferences, DashboardTab } from '../services/analyticsService';

/**
 * docked: a panel beside the current page; collapsed: a narrow strip;
 * maximized: the whole workspace; minimized: only an indicator in the header.
 */
export type AnalyticsMode = 'docked' | 'collapsed' | 'maximized' | 'minimized';

/** Narrowest docked Analytics panel. */
export const MIN_ANALYTICS = 400;
export const COLLAPSED_WIDTH = 44;

/** What to show when Analytics opens from elsewhere (e.g. "Analyze" on a chat answer). */
export interface AnalyticsFocus {
  runIds?: number[];
  jobIds?: number[];
  tab?: DashboardTab;
}

export interface AnalyticsWorkspace {
  open: boolean;
  mode: AnalyticsMode;
  /** Panel width in pixels; `null` = half of the workspace. */
  width: number | null;
  /** The dashboard's working state (scope, filters, ranking, options), saved in Rust. */
  prefs: AnalyticsPreferences | null;
  update: (change: (prefs: AnalyticsPreferences) => AnalyticsPreferences) => void;
  /** Jobs ticked in the jobs table (not yet the scope). */
  selectedJobs: number[];
  setSelectedJobs: (ids: number[]) => void;
  /** Analyzed jobs, shown by the minimized indicator. */
  jobCount: number | null;
  setJobCount: (count: number | null) => void;
  openAnalytics: (focus?: AnalyticsFocus) => void;
  close: () => void;
  setMode: (mode: AnalyticsMode) => void;
  setWidth: (width: number) => void;
}

export const AnalyticsContext = createContext<AnalyticsWorkspace | null>(null);

export function useAnalytics(): AnalyticsWorkspace {
  const analytics = useContext(AnalyticsContext);
  if (!analytics) throw new Error('useAnalytics must be used inside AnalyticsProvider');
  return analytics;
}

export function useOptionalAnalytics(): AnalyticsWorkspace | null {
  return useContext(AnalyticsContext);
}
