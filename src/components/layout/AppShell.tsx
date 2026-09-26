import { lazy, Suspense, useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react';

import { COLLAPSED_WIDTH, MIN_ANALYTICS, useAnalytics } from '../../app/analytics';
import { useBrowser } from '../../app/browser';
import { useNavigation } from '../../app/navigation';
import { BrowserPanel } from '../browser/BrowserPanel';
import { SIDEBAR_SHORTCUT } from '../../hooks/useSidebar';
import { ChartIcon, GlobeIcon, PanelIcon } from '../icons';
import { BrandMark } from '../ui/BrandMark';
import { IconButton } from '../ui/IconButton';
import { ThemeToggle } from './ThemeToggle';

// The dashboard (and its charting library) loads when Analytics first opens.
const AnalyticsPanel = lazy(() => import('../analytics/AnalyticsPanel').then((m) => ({ default: m.AnalyticsPanel })));

interface AppShellProps {
  sidebar: ReactNode;
  /** The sidebar is collapsed to an icon rail. */
  sidebarCollapsed: boolean;
  onToggleSidebar: () => void;
  children: ReactNode;
}

/** Space always kept for the current page next to open workspace panels. */
const MIN_CONTENT = 360;
const MIN_BROWSER = 340;
const SIDEBAR = 232;

/**
 * Top-level window layout: title bar, sidebar, the current page and, on
 * demand, the Analytics and browser workspace panels beside it:
 *
 *   browser docked right:  page | Analytics | browser
 *   browser docked left:   browser | page | Analytics
 *
 * When the window is too narrow for all three, the page steps aside and the
 * panels share the space; navigating brings the page back.
 */
export function AppShell({ sidebar, sidebarCollapsed, onToggleSidebar, children }: AppShellProps) {
  const browser = useBrowser();
  const analytics = useAnalytics();
  const { view } = useNavigation();
  const workspaceRef = useRef<HTMLDivElement>(null);
  const [space, setSpace] = useState(() => Math.max(0, window.innerWidth - SIDEBAR));

  useLayoutEffect(() => {
    const el = workspaceRef.current;
    if (!el) return;
    const observer = new ResizeObserver(() => setSpace(el.getBoundingClientRect().width));
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  const browserMax = browser.open && browser.mode === 'maximized';
  const showAnalytics = analytics.open && analytics.mode !== 'minimized';
  const analyticsCollapsed = showAnalytics && (analytics.mode === 'collapsed' || browserMax);
  const analyticsMax = showAnalytics && !analyticsCollapsed && analytics.mode === 'maximized';

  const browserWidth = !browser.open
    ? 0
    : browser.mode === 'collapsed'
      ? COLLAPSED_WIDTH
      : browser.mode === 'maximized'
        ? 0
        : Math.max(browser.width, MIN_BROWSER);
  let analyticsWidth = COLLAPSED_WIDTH;
  let crowded = false;
  if (showAnalytics && !analyticsCollapsed && !analyticsMax) {
    const preferred = analytics.width ?? Math.round(space * 0.5);
    const room = space - browserWidth - MIN_CONTENT;
    if (room >= MIN_ANALYTICS) {
      analyticsWidth = Math.min(Math.max(preferred, MIN_ANALYTICS), room);
    } else {
      crowded = true;
    }
  }
  const mainHidden = browserMax || analyticsMax || crowded;

  // Going to another page while the panels fill the window brings it back.
  const viewKey = JSON.stringify(view);
  const lastView = useRef(viewKey);
  const { setMode } = analytics;
  useEffect(() => {
    if (lastView.current === viewKey) return;
    lastView.current = viewKey;
    if (crowded || analyticsMax) setMode('collapsed');
  }, [viewKey, crowded, analyticsMax, setMode]);

  const panel = showAnalytics && (
    <Suspense fallback={<aside className="analytics" style={{ width: analyticsWidth }} aria-label="Analytics" aria-busy="true" />}>
      <AnalyticsPanel width={analyticsWidth} fill={crowded} collapsed={analyticsCollapsed} />
    </Suspense>
  );
  const browserPanel = browser.open && <BrowserPanel />;

  return (
    <div className="app-shell">
      {/* The whole header moves the window; buttons and links inside stay clickable. */}
      <header className="app-shell__topbar" data-tauri-drag-region="deep">
        <div className="app-shell__brand">
          <BrandMark size={26} />
          <span className="app-shell__wordmark">ReMa</span>
        </div>
        <IconButton
          label={`${sidebarCollapsed ? 'Show' : 'Hide'} sidebar (${SIDEBAR_SHORTCUT})`}
          aria-controls="app-sidebar"
          className="icon-button--small app-shell__sidebar-toggle"
          onClick={onToggleSidebar}
        >
          <PanelIcon side="left" />
        </IconButton>
        <div className="app-shell__tools">
          {analytics.open && analytics.mode === 'minimized' ? (
            <button
              type="button"
              className="analytics-button analytics-button--minimized"
              title="Restore Analytics"
              onClick={() => analytics.setMode('docked')}
            >
              <ChartIcon className="analytics-button__icon" />
              Analytics
              {analytics.jobCount != null && <span className="analytics-button__count">{analytics.jobCount}</span>}
            </button>
          ) : (
            <button
              type="button"
              className={`analytics-button${analytics.open ? ' analytics-button--on' : ''}`}
              aria-pressed={analytics.open}
              title={analytics.open ? 'Close Analytics' : 'Open the analytical dashboard'}
              onClick={() => (analytics.open ? analytics.close() : analytics.openAnalytics())}
            >
              <ChartIcon className="analytics-button__icon" />
              Analytics
            </button>
          )}
          <IconButton
            label={browser.open ? 'Hide browser' : 'Browser'}
            aria-pressed={browser.open}
            className="icon-button--small"
            onClick={browser.open ? browser.close : browser.show}
          >
            <GlobeIcon />
          </IconButton>
          <ThemeToggle />
        </div>
      </header>
      <div className="app-shell__body">
        {sidebar}
        <div ref={workspaceRef} className="app-shell__workspace">
          {browser.dock === 'left' && browserPanel}
          {/* Kept mounted while hidden, so nothing on the page is lost. */}
          <main className="app-shell__main" hidden={mainHidden}>
            {children}
          </main>
          {panel}
          {browser.dock === 'right' && browserPanel}
        </div>
      </div>
    </div>
  );
}
