import type { ReactNode } from 'react';

import { BrandMark } from '../ui/BrandMark';

interface AppShellProps {
  sidebar: ReactNode;
  children: ReactNode;
}

/** Top-level window layout: title bar, sidebar and main content area. */
export function AppShell({ sidebar, children }: AppShellProps) {
  return (
    <div className="app-shell">
      <header className="app-shell__topbar">
        <div className="app-shell__brand">
          <BrandMark />
          <span className="app-shell__wordmark">ReMa</span>
        </div>
      </header>
      <div className="app-shell__body">
        {sidebar}
        <main className="app-shell__main">{children}</main>
      </div>
    </div>
  );
}
