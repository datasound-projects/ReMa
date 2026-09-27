import { useState } from 'react';

import { useRemaMcp } from '../../hooks/useRemaMcp';
import { toApiError } from '../../services/ipc';
import {
  clearRemaMcpCache,
  remaMcpStatus,
  setRemaMcpEnabled,
  type Readiness,
  type RemaMcpStatus,
} from '../../services/remaMcpService';
import { ChevronDownIcon, ChevronRightIcon, SearchIcon } from '../icons';
import { Dialog } from '../ui/Dialog';
import { StatusIndicator, type StatusTone } from '../ui/StatusIndicator';
import { Switch } from '../ui/Switch';

const READINESS: Record<Readiness, { tone: StatusTone; label: string }> = {
  ready: { tone: 'ready', label: 'Ready' },
  offline: { tone: 'error', label: 'Offline' },
  error: { tone: 'error', label: 'Error' },
  disabled: { tone: 'idle', label: 'Off' },
};

/** Two confirmations before ReMa MCP is turned off. */
type Step = 'warning' | 'confirm' | null;

/** An action's result, shown until a newer status arrives from the backend. */
type Applied = { after: RemaMcpStatus | null; status: RemaMcpStatus };

/**
 * ReMa MCP in Settings → MCP → Built-in: on by default, in every chat. It
 * can be turned off (after two confirmations) and on again with one click;
 * it cannot be edited or removed.
 */
export function RemaMcpCard() {
  const remote = useRemaMcp();
  const latest = remote.state.status === 'success' ? remote.state.data : null;
  const [override, setOverride] = useState<Applied | null>(null);
  const [step, setStep] = useState<Step>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [showSources, setShowSources] = useState(false);
  const [checked, setChecked] = useState<string | null>(null);

  const status = override && override.after === latest ? override.status : (latest ?? override?.status ?? null);

  const run = async (action: () => Promise<RemaMcpStatus>) => {
    setBusy(true);
    setError(null);
    try {
      const next = await action();
      setOverride({ after: latest, status: next });
      remote.refresh();
      return next;
    } catch (err) {
      setError(toApiError(err).message);
      return null;
    } finally {
      setBusy(false);
    }
  };

  const onToggle = (enabled: boolean) => {
    if (enabled) {
      // Turning it back on takes one click.
      void run(() => setRemaMcpEnabled(true));
    } else {
      setStep('warning');
    }
  };

  const check = async () => {
    const next = await run(() => remaMcpStatus(true));
    if (next && next.enabled) {
      setChecked(
        next.tools.length > 0
          ? `Working: ${next.tools.length} tools available (${next.tools.join(', ')}).`
          : 'The built-in server did not list its tools.',
      );
    }
  };

  const readiness = status ? READINESS[status.readiness] : null;

  return (
    <div className="provider mcp-server rema-mcp" aria-labelledby="rema-mcp-name">
      <div className="provider__row">
        <SearchIcon className="mcp-server__icon" aria-hidden="true" />
        <div className="mcp-server__main">
          <span className="provider__name" id="rema-mcp-name">
            ReMa MCP — Built-in job search
          </span>
          <span className="mcp-server__type">Job search and job descriptions</span>
        </div>
        {readiness && <StatusIndicator tone={readiness.tone} label={readiness.label} live />}
        <Switch
          checked={status?.enabled ?? false}
          disabled={status === null || busy}
          onChange={onToggle}
          aria-label="ReMa MCP enabled"
        />
      </div>

      {remote.state.status === 'error' && !status && (
        <p className="form-error" role="alert">
          {remote.state.error.message}
        </p>
      )}
      {status && (
        <div className="rema-mcp__body">
          <p className="rema-mcp__message">{status.message}</p>
          {status.enabled && (
            <p className="rema-mcp__hint">
              Offered in every chat automatically. It searches only when a model needs it for your request; searches
              go from this computer to the job sources (and, when used, your model provider or optional search
              service), never through a ReMa server.
            </p>
          )}
          <div className="rema-mcp__actions">
            <button
              type="button"
              className="button button--ghost button--small"
              aria-expanded={showSources}
              onClick={() => setShowSources(!showSources)}
            >
              {showSources ? <ChevronDownIcon className="button__icon" /> : <ChevronRightIcon className="button__icon" />}
              Sources
            </button>
            {status.enabled && (
              <button type="button" className="button button--ghost button--small" disabled={busy} onClick={() => void check()}>
                Check
              </button>
            )}
            <button
              type="button"
              className="button button--ghost button--small"
              disabled={busy || (status.cachedJobs === 0 && status.cachedSearches === 0)}
              title="Removes ReMa MCP's cached jobs and searches. Chats, Analytics and your Profile are kept."
              onClick={() => {
                setChecked(null);
                void run(clearRemaMcpCache);
              }}
            >
              Clear cache ({status.cachedJobs} {status.cachedJobs === 1 ? 'job' : 'jobs'})
            </button>
          </div>
          {checked && (
            <p className="rema-mcp__checked" role="status">
              {checked}
            </p>
          )}
          {showSources && (
            <ul className="rema-mcp__sources">
              {status.sources.map((s) => (
                <li key={s.name}>
                  <span className="rema-mcp__source-name">{s.name}</span>
                  <span className="rema-mcp__source-access">{s.access}</span>
                  {!s.usable && <span className="rema-mcp__source-off">Not usable now</span>}
                  <span className="rema-mcp__source-note">{s.lastError ? `Last error: ${s.lastError}` : s.note}</span>
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
      {error && (
        <p className="form-error" role="alert">
          {error}
        </p>
      )}

      {step === 'warning' && (
        <Dialog
          title="Disable ReMa MCP?"
          onClose={() => setStep(null)}
          actions={
            <>
              <button type="button" className="button button--secondary" onClick={() => setStep(null)}>
                Cancel
              </button>
              <button type="button" className="button button--primary" onClick={() => setStep('confirm')}>
                Continue
              </button>
            </>
          }
        >
          <p className="dialog__text">Its built-in job-search and job-description tools will become unavailable.</p>
        </Dialog>
      )}
      {step === 'confirm' && (
        <Dialog
          title="Confirm disabling ReMa MCP?"
          onClose={() => setStep(null)}
          actions={
            <>
              <button type="button" className="button button--secondary" onClick={() => setStep(null)}>
                Keep enabled
              </button>
              <button
                type="button"
                className="button button--danger"
                disabled={busy}
                onClick={() => {
                  setStep(null);
                  setChecked(null);
                  void run(() => setRemaMcpEnabled(false));
                }}
              >
                Disable ReMa MCP
              </button>
            </>
          }
        >
          <p className="dialog__text">You can enable it again anytime in Settings.</p>
        </Dialog>
      )}
    </div>
  );
}
