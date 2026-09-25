import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';

import {
  BrowserContext,
  type BrowserDock,
  type BrowserMode,
  type BrowserRequest,
  type BrowserWorkspace,
} from '../../app/browser';
import { useBackendEvent } from '../../hooks/useBackendEvent';
import {
  closeBrowser,
  getBrowserStatus,
  type BrowserStatus,
} from '../../services/browserService';
import { backendEvents } from '../../services/events';

const DEFAULT_WIDTH = 560;
const LAYOUT_KEY = 'rema.browser.layout';

interface StoredLayout {
  dock: BrowserDock;
  width: number;
}

/** Dock side and width are a per-device preference. */
function loadLayout(): StoredLayout {
  try {
    const saved = JSON.parse(localStorage.getItem(LAYOUT_KEY) ?? '{}') as Partial<StoredLayout>;
    return {
      dock: saved.dock === 'left' ? 'left' : 'right',
      width: typeof saved.width === 'number' ? saved.width : DEFAULT_WIDTH,
    };
  } catch {
    return { dock: 'right', width: DEFAULT_WIDTH };
  }
}

const CLOSED: BrowserStatus = { open: false, url: null, title: null, loading: false };

/** Owns the browser workspace state shared by the panel, links and dialogs. */
export function BrowserProvider({ children }: { children: ReactNode }) {
  const [open, setOpen] = useState(false);
  const [status, setStatus] = useState<BrowserStatus>(CLOSED);
  const [request, setRequest] = useState<BrowserRequest | null>(null);
  const [layout, setLayout] = useState<StoredLayout>(loadLayout);
  const [mode, setMode] = useState<BrowserMode>('docked');
  const [covers, setCovers] = useState(0);
  const lastUrl = useRef<string | null>(null);
  const seq = useRef(0);

  // After a frontend reload the webview may still be open.
  useEffect(() => {
    void getBrowserStatus()
      .then((s) => {
        setStatus(s);
        if (s.open) setOpen(true);
      })
      .catch(() => {});
  }, []);

  useBackendEvent(backendEvents.browserChanged, (next) => {
    setStatus(next);
    if (next.url) lastUrl.current = next.url;
  });

  useEffect(() => {
    try {
      localStorage.setItem(LAYOUT_KEY, JSON.stringify(layout));
    } catch {
      // Storage unavailable: the layout just isn't remembered.
    }
  }, [layout]);

  const openUrl = useCallback((url: string) => {
    seq.current += 1;
    setRequest({ url, seq: seq.current });
    setOpen(true);
    setMode((m) => (m === 'collapsed' ? 'docked' : m));
  }, []);

  const show = useCallback(() => {
    setOpen(true);
    setMode((m) => (m === 'collapsed' ? 'docked' : m));
    if (!status.open && lastUrl.current) {
      seq.current += 1;
      setRequest({ url: lastUrl.current, seq: seq.current });
    }
  }, [status.open]);

  const close = useCallback(() => {
    setOpen(false);
    setRequest(null);
    setMode('docked');
    void closeBrowser().catch(() => {});
  }, []);

  const requestDone = useCallback(
    (done: number) => setRequest((r) => (r && r.seq === done ? null : r)),
    [],
  );

  const cover = useCallback(() => {
    setCovers((n) => n + 1);
    let released = false;
    return () => {
      if (released) return;
      released = true;
      setCovers((n) => n - 1);
    };
  }, []);

  const value = useMemo<BrowserWorkspace>(
    () => ({
      open,
      status,
      request,
      dock: layout.dock,
      mode,
      width: layout.width,
      covered: covers > 0,
      openUrl,
      show,
      close,
      requestDone,
      setDock: (dock) => setLayout((l) => ({ ...l, dock })),
      setMode,
      setWidth: (width) => setLayout((l) => ({ ...l, width })),
      cover,
    }),
    [open, status, request, layout, mode, covers, openUrl, show, close, requestDone, cover],
  );

  return <BrowserContext value={value}>{children}</BrowserContext>;
}
