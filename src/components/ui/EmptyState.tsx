import type { ReactNode } from 'react';

interface EmptyStateProps {
  icon?: ReactNode;
  title: string;
  children?: ReactNode;
  /** Buttons for the next step. */
  actions?: ReactNode;
  /** Smaller padding, for use inside a panel. */
  compact?: boolean;
  /** Drawn as its own dashed card. */
  framed?: boolean;
}

/** What a place is for, and what to do first, when it has nothing to show yet. */
export function EmptyState({ icon, title, children, actions, compact, framed }: EmptyStateProps) {
  const className = ['empty-state', compact && 'empty-state--compact', framed && 'empty-state--panel']
    .filter(Boolean)
    .join(' ');
  return (
    <div className={className}>
      {icon && (
        <span className="empty-state__icon" aria-hidden="true">
          {icon}
        </span>
      )}
      <p className="empty-state__title">{title}</p>
      {children && <p className="empty-state__text">{children}</p>}
      {actions && <div className="empty-state__actions">{actions}</div>}
    </div>
  );
}

/** A quiet "working on it" row. */
export function LoadingState({ label = 'Loading…' }: { label?: string }) {
  return (
    <div className="loading-state" role="status">
      <span className="spinner" aria-hidden="true" />
      {label}
    </div>
  );
}
