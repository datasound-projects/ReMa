import type { ReactNode } from 'react';

import { useBrowser } from '../../app/browser';
import { BrowserPanel } from '../browser/BrowserPanel';
import { GlobeIcon } from '../icons';
import { BrandMark } from '../ui/BrandMark';
import { IconButton } from '../ui/IconButton';

interface AppShellProps {
  sidebar: ReactNode;
  children: ReactNode;
}

/**
 * Top-level window layout: title bar, sidebar, main content area and, on
 * demand, the browser workspace docked beside the content.
 */
export function AppShell({ sidebar, children }: AppShellProps) {
  const browser = useBrowser();
  const maximized = browser.open && browser.mode === 'maximized';

  return (
    <div className="app-shell">
      {/* The whole header moves the window; buttons and links inside stay clickable. */}
      <header className="app-shell__topbar" data-tauri-drag-region="deep">
        <div className="app-shell__brand">
          <BrandMark />
          <span className="app-shell__wordmark">ReMa</span>
        </div>
        <div className="app-shell__tools">
          <IconButton
            label={browser.open ? 'Hide browser' : 'Browser'}
            aria-pressed={browser.open}
            className="icon-button--small"
            onClick={browser.open ? browser.close : browser.show}
          >
            <GlobeIcon />
          </IconButton>
        </div>
      </header>
      <div className="app-shell__body">
        {sidebar}
        <div className={`app-shell__workspace app-shell__workspace--${browser.dock}`}>
          {/* Kept mounted while the browser is maximized, so nothing is lost. */}
          <main className="app-shell__main" hidden={maximized}>
            {children}
          </main>
          {browser.open && <BrowserPanel />}
        </div>
      </div>
    </div>
  );
}
