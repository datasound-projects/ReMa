import { useTheme } from '../../hooks/useTheme';
import { setTheme } from '../../lib/theme';
import { MoonIcon, SunIcon } from '../icons';

/** A quiet sun/moon switch at the far right of the title bar. */
export function ThemeToggle() {
  const theme = useTheme();
  const next = theme === 'dark' ? 'light' : 'dark';
  const label = theme === 'dark' ? 'Switch to light mode' : 'Switch to dark mode';
  return (
    <button
      type="button"
      className="theme-toggle"
      aria-label={label}
      title={label}
      onClick={() => setTheme(next)}
    >
      {theme === 'dark' ? <SunIcon /> : <MoonIcon />}
    </button>
  );
}
