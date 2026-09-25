export type StatusTone = 'ready' | 'pending' | 'error';

interface StatusIndicatorProps {
  tone: StatusTone;
  label: string;
}

/** Colored dot followed by a short status label. */
export function StatusIndicator({ tone, label }: StatusIndicatorProps) {
  return (
    <span className={`status-indicator status-indicator--${tone}`} role="status">
      <span className="status-indicator__dot" aria-hidden="true" />
      {label}
    </span>
  );
}
