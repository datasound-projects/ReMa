import { isTauri } from '@tauri-apps/api/core';

/**
 * Marks the document root with the desktop platform so CSS can adapt the
 * window chrome, e.g. `:root[data-platform='macos']`.
 *
 * Only macOS needs this today: its title bar is an overlay (see
 * `tauri.conf.json`), so the header must leave room for the traffic lights.
 */
export function applyPlatformAttributes(): void {
  const isMacDesktop = isTauri() && /Macintosh|Mac OS X/.test(navigator.userAgent);
  if (isMacDesktop) document.documentElement.dataset.platform = 'macos';
}
