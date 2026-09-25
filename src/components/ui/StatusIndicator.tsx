export type StatusTone = 'ready' | 'pending' | 'error';

interface StatusIndicatorProps {
  tone: StatusTone;
  label: string;
  /**
   * Announce label changes to screen readers (`role="status"`). Off by
   * default so screens with many indicators stay quiet; enable it only for
   * the one status the user is waiting on.
   */
  live?: boolean;
}

/** Colored dot followed by a short status label. */
export function StatusIndicator({ tone, label, live = false }: StatusIndicatorProps) {
  return (
    <span
      className={`status-indicator status-indicator--${tone}`}
      role={live ? 'status' : undefined}
    >
      <span className="status-indicator__dot" aria-hidden="true" />
      {label}
    </span>
  );
}
