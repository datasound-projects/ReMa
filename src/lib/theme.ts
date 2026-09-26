import { setAppearance } from '../services/systemService';

/** ReMa's color theme. Light is the default; Dark is opt-in. */
export type Theme = 'light' | 'dark';

const KEY = 'rema.theme';
const listeners = new Set<() => void>();
let current: Theme = read();

function read(): Theme {
  try {
    return localStorage.getItem(KEY) === 'dark' ? 'dark' : 'light';
  } catch {
    return 'light';
  }
}

function apply(theme: Theme) {
  const root = document.documentElement;
  // Colors change at once instead of every control animating its own.
  root.classList.add('theme-switching');
  root.dataset.theme = theme;
  requestAnimationFrame(() => requestAnimationFrame(() => root.classList.remove('theme-switching')));
}

export function getTheme(): Theme {
  return current;
}

/** Switches the theme, remembers it and tells the window (native menus, first frame). */
export function setTheme(theme: Theme) {
  current = theme;
  apply(theme);
  try {
    localStorage.setItem(KEY, theme);
  } catch {
    // Not remembered this time; the theme still switches.
  }
  listeners.forEach((listener) => listener());
  void setAppearance(theme).catch(() => {});
}

export function subscribeTheme(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** At startup: the page already shows the saved theme (theme-init.js); sync the window. */
export function initTheme() {
  document.documentElement.dataset.theme = current;
  void setAppearance(current).catch(() => {});
}
