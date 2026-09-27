import { useCallback, useState } from 'react';

import { useAsyncData } from '../../hooks/useAsyncData';
import { useBackendEvent } from '../../hooks/useBackendEvent';
import {
  careerSearchStatus,
  checkCareerSearch,
  type CareerSearchStatus,
  type RouteReport,
} from '../../services/careerSearchService';
import { backendEvents } from '../../services/events';
import { toApiError } from '../../services/ipc';
import { Collapsible } from '../ui/Collapsible';
import { HelpTip } from '../ui/HelpTip';
import { StatusIndicator } from '../ui/StatusIndicator';
import { AdditionalSearchService } from './AdditionalSearchService';

const HELP =
  'When you ask for current jobs, companies, people or salaries, ReMa searches before it answers: its own job sources (employers’ job boards and public job boards), company websites and public data, and the selected model’s own web search when it has one. Local models get the same search as tools. Searches run in the background — no browser windows open — and a result is only shown with its source.';

function when(ms: number): string {
  return new Date(ms).toLocaleString(undefined, { hour: '2-digit', minute: '2-digit', day: 'numeric', month: 'short' });
}

function describeReport(r: RouteReport): string {
  const parts = [r.route === 'none' ? 'No route answered' : r.route, `${r.results} result${r.results === 1 ? '' : 's'}`];
  parts.push(`${(r.durationMs / 1000).toFixed(1)} s`);
  if (r.failed.length > 0) parts.push(`${r.failed.length} source${r.failed.length === 1 ? '' : 's'} unavailable`);
  return parts.join(' · ');
}

/**
 * Settings → Career Search: automatic, with no setup. Shows the routes the
 * selected model has, a health check, recent searches, and (under
 * Advanced) an optional additional search service.
 */
export function CareerSearchSection() {
  const load = useCallback(() => careerSearchStatus(), []);
  const data = useAsyncData(load);
  useBackendEvent(backendEvents.providersChanged, data.refresh);
  const [checked, setChecked] = useState<CareerSearchStatus | null>(null);
  const [checking, setChecking] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const status = checked ?? (data.state.status === 'success' ? data.state.data : null);

  const check = async () => {
    setChecking(true);
    setError(null);
    try {
      setChecked(await checkCareerSearch());
    } catch (err) {
      setError(toApiError(err).message);
    } finally {
      setChecking(false);
    }
  };

  return (
    <section className="section" aria-labelledby="career-search-heading" id="settings-career-search">
      <div className="section__head">
        <div className="section__heading">
          <h2 id="career-search-heading" className="section__title mcp__title">
            Career Search
            <HelpTip text={HELP} label="How does ReMa search?" />
          </h2>
          <p className="section__description">
            ReMa automatically uses available model-native and built-in career search for current jobs and
            professional research. No API key required.
          </p>
        </div>
        <StatusIndicator tone="ready" label="Automatic" />
      </div>
      <div className="panel career-search">
        {data.state.status === 'error' && !status && (
          <p className="form-error" role="alert">
            {data.state.error.message}
          </p>
        )}
        {status && (
          <>
            {status.model && (
              <p className="career-search__model">
                Selected model: <strong>{status.model}</strong>
                {status.modelSearch ? ` — with ${status.modelSearch}` : ' — ReMa searches for it'}
              </p>
            )}
            <ul className="career-search__routes" aria-label="Search routes">
              {status.routes.map((route) => (
                <li key={route.name} className="career-search__route">
                  <StatusIndicator
                    tone={route.available ? 'ready' : 'idle'}
                    label={route.name}
                  />
                  <span className="career-search__detail">{route.detail}</span>
                </li>
              ))}
            </ul>
            <div className="career-search__actions">
              <button
                type="button"
                className="button button--secondary button--small"
                disabled={checking}
                onClick={() => void check()}
              >
                {checking ? 'Checking…' : 'Check now'}
              </button>
              {!checking && checked === null && (
                <span className="career-search__hint">Nothing to set up; the check only confirms sources answer.</span>
              )}
            </div>
            {status.checked.length > 0 && (
              <ul className="career-search__checks" role="status">
                {status.checked.map((c) => (
                  <li key={c.name}>
                    <StatusIndicator tone={c.ok ? 'ready' : 'pending'} label={c.name} />
                    <span className="career-search__detail">{c.detail}</span>
                  </li>
                ))}
              </ul>
            )}
            {status.recent.length > 0 && (
              <Collapsible title="Recent searches" count={status.recent.length}>
                <ul className="career-search__recent">
                  {status.recent.map((r) => (
                    <li key={`${r.at}-${r.route}`}>
                      <span className="career-search__when">{when(r.at)}</span>
                      <span>{describeReport(r)}</span>
                      {r.scopes.length > 0 && <span className="career-search__scope">{r.scopes.join(', ')}</span>}
                    </li>
                  ))}
                </ul>
              </Collapsible>
            )}
          </>
        )}
        {error && (
          <p className="form-error" role="alert">
            {error}
          </p>
        )}
        <Collapsible title="Advanced (optional)">
          <AdditionalSearchService />
        </Collapsible>
      </div>
    </section>
  );
}
