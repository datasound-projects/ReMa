/**
 * Placeholder rows shown while a Settings section loads, so a section
 * never looks empty before its first answer arrives (the keychain or a
 * provider can take a few seconds on a cold start).
 */
export function LoadingRows({ count = 2, label }: { count?: number; label: string }) {
  return (
    <div className="loading-rows" role="status" aria-live="polite" aria-label={label}>
      {Array.from({ length: count }, (_, i) => (
        <div key={i} className="loading-rows__row" aria-hidden="true">
          <span className="loading-rows__bar loading-rows__bar--title" />
          <span className="loading-rows__bar loading-rows__bar--text" />
        </div>
      ))}
      <span className="visually-hidden">{label}</span>
    </div>
  );
}

/** An error with a way to try again, in place of a section's content. */
export function LoadFailed({ message, onRetry }: { message: string; onRetry: () => void }) {
  return (
    <div className="load-failed" role="alert">
      <p className="form-error">{message}</p>
      <button type="button" className="button button--ghost" onClick={onRetry}>
        Try again
      </button>
    </div>
  );
}
