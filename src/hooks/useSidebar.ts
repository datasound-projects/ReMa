import { useCallback, useEffect, useState } from 'react';

const KEY = 'rema.sidebar.collapsed';
const isMac = /Macintosh|Mac OS X/.test(navigator.userAgent);

/** The keyboard shortcut that shows or hides the sidebar. */
export const SIDEBAR_SHORTCUT = isMac ? '⌘B' : 'Ctrl+B';

function load(): boolean {
  try {
    return localStorage.getItem(KEY) === '1';
  } catch {
    return false;
  }
}

/**
 * Whether the sidebar is collapsed to an icon rail. Remembered on this
 * computer; ⌘B (macOS) or Ctrl+B toggles it.
 */
export function useSidebar() {
  const [collapsed, setCollapsed] = useState(load);
  const toggle = useCallback(() => setCollapsed((c) => !c), []);

  useEffect(() => {
    try {
      localStorage.setItem(KEY, collapsed ? '1' : '0');
    } catch {
      // Not remembered this time; the sidebar still works.
    }
  }, [collapsed]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key.toLowerCase() !== 'b' || event.altKey || event.shiftKey) return;
      // ⌘B on macOS (Ctrl+B moves the caret there); Ctrl+B elsewhere.
      const mod = isMac ? event.metaKey && !event.ctrlKey : event.ctrlKey && !event.metaKey;
      if (!mod) return;
      event.preventDefault();
      toggle();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [toggle]);

  return { collapsed, toggle };
}
