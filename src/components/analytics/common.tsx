import type { ReactNode } from 'react';

import { useOpenLink } from '../../hooks/useOpenLink';
import { PRIORITIES, STATE_ICONS, STATES } from '../../lib/analytics';
import type { MatchState, PriorityLevel } from '../../services/analyticsService';
import { openExternalUrl } from '../../services/systemService';
import { ExternalIcon } from '../icons';

/** A Profile state: glyph + word, never color alone. */
export function StateBadge({ state, title }: { state: MatchState | null | undefined; title?: string | null }) {
  if (!state) return <span className="a-muted">—</span>;
  return (
    <span className={`state state--${state}`} title={title ?? undefined}>
      <span className="state__icon" aria-hidden="true">
        {STATE_ICONS[state]}
      </span>
      {STATES[state]}
    </span>
  );
}

export function PriorityBadge({ level }: { level: PriorityLevel }) {
  return <span className={`priority priority--${level}`}>{PRIORITIES[level]}</span>;
}

export function WebLink({ url, children, className }: { url: string; children: ReactNode; className?: string }) {
  const open = useOpenLink();
  return (
    <span className="web-link">
      <a
        href={url}
        className={className ?? 'a-link'}
        title="Open in ReMa (Ctrl/⌘-click: external browser)"
        onClick={(e) => {
          e.preventDefault();
          open(url, e);
        }}
      >
        {children}
      </a>
      <button
        type="button"
        className="web-link__external"
        aria-label="Open in external browser"
        title="Open in external browser"
        onClick={() => void openExternalUrl(url).catch(() => {})}
      >
        <ExternalIcon />
      </button>
    </span>
  );
}

export function Notes({ notes }: { notes: string[] }) {
  if (notes.length === 0) return null;
  return (
    <ul className="a-notes">
      {notes.map((n) => (
        <li key={n}>{n}</li>
      ))}
    </ul>
  );
}

/** Deterministic summary lines from Rust. */
export function Summary({ lines }: { lines: string[] }) {
  if (lines.length === 0) return null;
  return (
    <div className="a-summary" role="note">
      {lines.map((line) => (
        <p key={line}>{line}</p>
      ))}
    </div>
  );
}

export function Stat({ label, value, hint }: { label: string; value: ReactNode; hint?: string }) {
  return (
    <div className="a-stat" title={hint}>
      <span className="a-stat__label">{label}</span>
      <span className="a-stat__value">{value}</span>
    </div>
  );
}

export function Loading({ busy, children }: { busy: boolean; children: ReactNode }) {
  return <div className={`a-view${busy ? ' a-view--busy' : ''}`}>{children}</div>;
}
