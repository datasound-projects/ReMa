import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';

import {
  AnalyticsContext,
  type AnalyticsFocus,
  type AnalyticsMode,
  type AnalyticsWorkspace,
} from '../../app/analytics';
import {
  getAnalyticsPreferences,
  saveAnalyticsPreferences,
  type AnalyticsPreferences,
} from '../../services/analyticsService';

const LAYOUT_KEY = 'rema.analytics.layout';

interface StoredLayout {
  open: boolean;
  mode: AnalyticsMode;
  width: number | null;
}

const MODES: AnalyticsMode[] = ['docked', 'collapsed', 'maximized', 'minimized'];

/** Panel layout is a per-device preference. */
function loadLayout(): StoredLayout {
  try {
    const saved = JSON.parse(localStorage.getItem(LAYOUT_KEY) ?? '{}') as Partial<StoredLayout>;
    return {
      open: saved.open === true,
      mode: MODES.includes(saved.mode as AnalyticsMode) ? (saved.mode as AnalyticsMode) : 'docked',
      width: typeof saved.width === 'number' ? saved.width : null,
    };
  } catch {
    return { open: false, mode: 'docked', width: null };
  }
}

/**
 * Owns the Analytics workspace: panel layout (local) and the dashboard's
 * working state (saved in Rust). Both survive navigating around ReMa, closing
 * the panel and restarting the app.
 */
export function AnalyticsProvider({ children }: { children: ReactNode }) {
  const [layout, setLayout] = useState<StoredLayout>(loadLayout);
  const [prefs, setPrefs] = useState<AnalyticsPreferences | null>(null);
  const [selectedJobs, setSelectedJobs] = useState<number[]>([]);
  const [jobCount, setJobCount] = useState<number | null>(null);
  const dirty = useRef(false);

  useEffect(() => {
    void getAnalyticsPreferences()
      .then((loaded) => setPrefs((current) => current ?? loaded))
      .catch(() => {});
  }, []);

  useEffect(() => {
    try {
      localStorage.setItem(LAYOUT_KEY, JSON.stringify(layout));
    } catch {
      // Storage unavailable: the layout just isn't remembered.
    }
  }, [layout]);

  // Save the working state shortly after it changes.
  useEffect(() => {
    if (!prefs || !dirty.current) return;
    const timer = window.setTimeout(() => {
      dirty.current = false;
      void saveAnalyticsPreferences(prefs).catch(() => {});
    }, 400);
    return () => window.clearTimeout(timer);
  }, [prefs]);

  const update = useCallback((change: (p: AnalyticsPreferences) => AnalyticsPreferences) => {
    dirty.current = true;
    setPrefs((p) => (p ? change(p) : p));
  }, []);

  const openAnalytics = useCallback(
    (focus?: AnalyticsFocus) => {
      setLayout((l) => ({ ...l, open: true, mode: l.mode === 'maximized' ? 'maximized' : 'docked' }));
      if (!focus) return;
      setSelectedJobs([]);
      update((p) => ({
        ...p,
        tab: focus.tab ?? p.tab,
        query: {
          ...p.query,
          scope: focus.jobIds
            ? { kind: 'jobs', runIds: [], jobIds: focus.jobIds }
            : focus.runIds
              ? { kind: 'searches', runIds: focus.runIds, jobIds: [] }
              : p.query.scope,
        },
      }));
    },
    [update],
  );

  const value = useMemo<AnalyticsWorkspace>(
    () => ({
      open: layout.open,
      mode: layout.mode,
      width: layout.width,
      prefs,
      update,
      selectedJobs,
      setSelectedJobs,
      jobCount,
      setJobCount,
      openAnalytics,
      close: () => setLayout((l) => ({ ...l, open: false, mode: 'docked' })),
      setMode: (mode) => setLayout((l) => ({ ...l, open: true, mode })),
      setWidth: (width) => setLayout((l) => ({ ...l, width })),
    }),
    [layout, prefs, update, selectedJobs, jobCount, openAnalytics],
  );

  return <AnalyticsContext value={value}>{children}</AnalyticsContext>;
}
