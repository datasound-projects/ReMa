import { useCallback, useState } from 'react';

import { LoadingRows } from '../ui/LoadingRows';
import { useAsyncData } from '../../hooks/useAsyncData';
import { useBackendEvent } from '../../hooks/useBackendEvent';
import {
  careerSearchStatus,
  checkCareerSearch,
  setAnswerMode,
  type AnswerMode,
  type CareerSearchStatus,
  type RouteReport,
  type RuntimeCapabilities,
} from '../../services/careerSearchService';
import { backendEvents } from '../../services/events';
import { toApiError } from '../../services/ipc';
import { Collapsible } from '../ui/Collapsible';
import { HelpTip } from '../ui/HelpTip';
import { StatusIndicator } from '../ui/StatusIndicator';
import { AdditionalSearchService } from './AdditionalSearchService';

const HELP =
  'When you ask for current jobs, companies, people or salaries, a model with its own web search (OpenAI, ChatGPT, Claude, Gemini 3) searches the web itself and writes the answer, as it does in ChatGPT and Claude; ReMa’s job sources are there as ReMa MCP tools. Models without a search of their own get ReMa’s search: its job sources (employers’ job boards and public job boards), company websites and public data. With ReMa verified search, ReMa searches first for every model and opens each posting it lists. Searches run in the background — no browser windows open.';

const MODES: { mode: AnswerMode; title: string; detail: string }[] = [
  {
    mode: 'model_search',
    title: 'The model’s own web search',
    detail:
      'As in ChatGPT and Claude: the model searches the web and writes the answer. Models without a search of their own get ReMa’s search.',
  },
  {
    mode: 'verified',
    title: 'ReMa verified search',
    detail:
      'ReMa searches its job sources and the model’s search first, opens every posting it lists, and the model writes about what was found.',
  },
];

function when(ms: number): string {
  return new Date(ms).toLocaleString(undefined, { hour: '2-digit', minute: '2-digit', day: 'numeric', month: 'short' });
}

const RUNTIMES: Record<RuntimeCapabilities['runtime'], string> = {
  openai_responses: 'OpenAI Responses API',
  codex_app_server: 'Codex (ChatGPT account)',
  anthropic_messages: 'Anthropic Messages API',
  gemini_api: 'Gemini API',
  unsloth_studio: 'Unsloth Studio',
  openai_compatible: 'Local or compatible server',
};

/**
 * One line on what the selected model's runtime can do for search (kept to
 * career sites only inside ReMa's verified search).
 */
function describeCapabilities(c: RuntimeCapabilities, mode: AnswerMode): string {
  const parts = [RUNTIMES[c.runtime]];
  if (c.nativeSearchAvailable) {
    parts.push(c.nativeDetail ? `own web search (${c.nativeDetail})` : 'own web search');
    if (c.nativeSearchLive && c.nativeDetail !== 'live') parts.push('live');
    if (c.nativeDomainFiltering && mode === 'verified') parts.push('kept to career sites');
    if (c.nativeCitations) parts.push('reports its sources');
  } else {
    parts.push('ReMa searches for it');
  }
  return parts.join(' · ');
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
  const [saving, setSaving] = useState(false);
  const [modeChoice, setModeChoice] = useState<AnswerMode | null>(null);

  const status = checked ?? (data.state.status === 'success' ? data.state.data : null);

  const changeMode = async (mode: AnswerMode) => {
    setSaving(true);
    setError(null);
    try {
      await setAnswerMode(mode);
      setModeChoice(mode);
      setChecked((c) => (c ? { ...c, answerMode: mode } : c));
      data.refresh();
    } catch (err) {
      setError(toApiError(err).message);
    } finally {
      setSaving(false);
    }
  };

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
            ReMa automatically uses the best available search capability for jobs and professional research.
            Nothing to set up. No API key required.
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
        {data.state.status === 'loading' && !status && <LoadingRows count={2} label="Loading career search…" />}
        {status && (
          <>
            {status.model && (
              <p className="career-search__model">
                Selected model: <strong>{status.model}</strong>
                {status.modelSearch ? ` — with ${status.modelSearch}` : ' — ReMa searches for it'}
              </p>
            )}
            {status.capabilities && (
              <p className="career-search__capabilities">
                {describeCapabilities(status.capabilities, modeChoice ?? status.answerMode)}
              </p>
            )}
            <fieldset className="career-search__mode">
              <legend className="field__label">How chats search the web</legend>
              {MODES.map(({ mode, title, detail }) => (
                <label key={mode} className="radio career-search__mode-option">
                  <input
                    type="radio"
                    name="answer-mode"
                    checked={(modeChoice ?? status.answerMode) === mode}
                    disabled={saving}
                    onChange={() => void changeMode(mode)}
                  />
                  <span>
                    <strong>{title}</strong>
                    <span className="career-search__detail">{detail}</span>
                  </span>
                </label>
              ))}
            </fieldset>
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
                <span className="career-search__hint">Nothing to set up. The check asks the sources and runs one real search with your default model, judged by the pages its provider returns.</span>
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
