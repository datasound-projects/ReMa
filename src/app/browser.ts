import { createContext, useContext, useEffect } from 'react';

import type { BrowserStatus } from '../services/browserService';

export type BrowserDock = 'left' | 'right';
export type BrowserMode = 'docked' | 'collapsed' | 'maximized';

/** A page waiting to be opened once the panel has measured its area. */
export interface BrowserRequest {
  url: string;
  seq: number;
}

export interface BrowserWorkspace {
  /** The panel is shown (docked, collapsed or maximized). */
  open: boolean;
  /** What the browser webview shows, reported by Rust. */
  status: BrowserStatus;
  request: BrowserRequest | null;
  dock: BrowserDock;
  mode: BrowserMode;
  width: number;
  /** ReMa overlays (dialogs, drags) that the web page must not cover. */
  covered: boolean;
  /** Opens a web page in the ReMa browser. */
  openUrl: (url: string) => void;
  /** Shows the panel again (last page), or an empty one. */
  show: () => void;
  close: () => void;
  requestDone: (seq: number) => void;
  setDock: (dock: BrowserDock) => void;
  setMode: (mode: BrowserMode) => void;
  setWidth: (width: number) => void;
  /** Hides the web page while something of ReMa's is on top; returns the release. */
  cover: () => () => void;
}

export const BrowserContext = createContext<BrowserWorkspace | null>(null);

export function useBrowser(): BrowserWorkspace {
  const browser = useContext(BrowserContext);
  if (!browser) throw new Error('useBrowser must be used inside BrowserProvider');
  return browser;
}

/** The browser workspace if there is one (dialogs can render without it). */
export function useOptionalBrowser(): BrowserWorkspace | null {
  return useContext(BrowserContext);
}

/**
 * Native web pages draw above ReMa's own interface, so dialogs hide the
 * page while they are open.
 */
export function useCoverBrowser(active = true) {
  const cover = useOptionalBrowser()?.cover;
  useEffect(() => (active && cover ? cover() : undefined), [active, cover]);
}
