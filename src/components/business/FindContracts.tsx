import { Fragment, useEffect, useState } from 'react';

import { useAction } from '../../hooks/useAction';
import { useBusinessRun } from '../../hooks/useBusiness';
import { formatDate, formatDateTime } from '../../lib/format';
import {
  findContracts,
  getLastBusinessResults,
  saveContract,
  type Comparison,
  type ContractCriteria,
  type ContractResult,
  type ContractResults,
  type ContractTerms,
  type MatchStatus,
  type RateUnit,
} from '../../services/businessService';
import { ChevronDownIcon, ChevronRightIcon, SearchIcon } from '../icons';
import { Switch } from '../ui/Switch';
import { Chips, Links, LocationsField, RunProgress, SearchedDetails, StatusBadge } from './common';
import { DEFAULT_CONTRACT_CRITERIA, durationSummary, RATE_UNITS, rateSummary } from './helpers';
import { ENGAGEMENT_LABELS, MATCH_LABELS } from './labels';

const EXAMPLES = [
  'Find Python/AI contracts in DACH lasting 1–6 months with rates above EUR 700/day.',
  'Freelance React projects, remote from Austria, at least EUR 90/hour.',
  'Interim data engineering assignments in Vienna or Munich starting within two months.',
];

function statusClass(status: MatchStatus) {
  return status === 'confirmed' ? 'badge badge--success' : status === 'needs_verification' ? 'badge badge--warning' : 'badge';
}

/**
 * Find Contract Work (B12–B14): advertised freelance, consulting, interim,
 * B2B and project engagements, with their terms as stated. Fixed-term
 * employment is kept apart from independent work.
 */
export function FindContracts({ onOpenOpportunity }: { onOpenOpportunity: (id: string) => void }) {
  const [query, setQuery] = useState('');
  const [criteria, setCriteria] = useState<ContractCriteria>(DEFAULT_CONTRACT_CRITERIA);
  const [useCriteria, setUseCriteria] = useState(false);
  const [results, setResults] = useState<ContractResults | null>(null);
  const run = useBusinessRun();

  useEffect(() => {
    let active = true;
    void getLastBusinessResults()
      .then((last) => {
        if (active && last.contracts) setResults((current) => current ?? last.contracts);
      })
      .catch(() => {});
    return () => {
      active = false;
    };
  }, []);

  const search = (text: string, withCriteria: boolean) => {
    if (run.running || (!text.trim() && !withCriteria)) return;
    void run.start(async (runId) => {
      const found = await findContracts({ runId, query: text.trim(), criteria: withCriteria ? criteria : null });
      setResults(found);
      setCriteria(found.criteria);
      return found;
    });
  };

  const markSaved = (key: string, opportunityId: string) =>
    setResults((r) => {
      if (!r) return r;
      const mark = (list: ContractResult[]) => list.map((c) => (c.key === key ? { ...c, opportunityId } : c));
      return { ...r, confirmed: mark(r.confirmed), needsVerification: mark(r.needsVerification), notMatching: mark(r.notMatching) };
    });

  const set = (patch: Partial<ContractCriteria>) => setCriteria({ ...criteria, ...patch });
  const num = (value: string) => (value.trim() === '' ? null : Number(value));

  return (
    <div className="biz-view">
      <form
        className="biz-search"
        onSubmit={(e) => {
          e.preventDefault();
          search(query, useCriteria);
        }}
      >
        <label htmlFor="biz-contracts-query" className="sr-only">
          Which contract work to look for
        </label>
        <textarea
          id="biz-contracts-query"
          className="input input--textarea biz-search__input"
          rows={2}
          placeholder="Python/AI contracts in DACH lasting 1–6 months with rates above EUR 700/day…"
          value={query}
          disabled={run.running}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && !e.shiftKey) {
              e.preventDefault();
              search(query, useCriteria);
            }
          }}
        />
        <div className="biz-search__bar">
          <button
            type="button"
            className="button button--ghost button--small"
            aria-expanded={useCriteria}
            onClick={() => setUseCriteria(!useCriteria)}
          >
            {useCriteria ? <ChevronDownIcon className="button__icon" /> : <ChevronRightIcon className="button__icon" />}
            {useCriteria ? 'Using these criteria' : 'Edit criteria'}
          </button>
          <span className="biz-search__spacer" />
          {run.running ? (
            <RunProgress status={run.status} fallback="Checking contract terms…" onStop={run.stop} />
          ) : (
            <button type="submit" className="button button--primary" disabled={!query.trim() && !useCriteria}>
              <SearchIcon className="button__icon" />
              Find contract work
            </button>
          )}
        </div>
        {useCriteria && (
          <div className="biz-criteria">
            <p className="form__hint">
              These criteria replace what ReMa reads from your request. A range straddling your minimum, an “up to”
              rate or a missing rate is never a confirmed match.
            </p>
            <label className="field">
              <span className="field__label">Skills</span>
              <input
                className="input"
                placeholder="Python / AI"
                value={criteria.skills}
                onChange={(e) => set({ skills: e.target.value })}
              />
            </label>
            <LocationsField value={criteria.locations} onChange={(locations) => set({ locations })} />
            <Switch checked={criteria.remoteOk} onChange={(remoteOk) => set({ remoteOk })}>
              <span className="biz-switch-label">Remote work is fine (only where the listing allows my country)</span>
            </Switch>
            <div className="biz-form-grid biz-form-grid--rate">
              <label className="field">
                <span className="field__label">Rate</span>
                <select
                  className="input"
                  value={criteria.rateComparison}
                  onChange={(e) => set({ rateComparison: e.target.value as Comparison })}
                >
                  <option value="above">Above (&gt;)</option>
                  <option value="at_least">At least (≥)</option>
                </select>
              </label>
              <label className="field">
                <span className="field__label">Amount</span>
                <input
                  className="input input--number"
                  type="number"
                  min={0}
                  value={criteria.minRate ?? ''}
                  onChange={(e) => set({ minRate: num(e.target.value) })}
                />
              </label>
              <label className="field">
                <span className="field__label">Currency</span>
                <input
                  className="input"
                  maxLength={3}
                  value={criteria.currency ?? ''}
                  onChange={(e) => set({ currency: e.target.value.toUpperCase() || null })}
                />
              </label>
              <label className="field">
                <span className="field__label">Unit</span>
                <select className="input" value={criteria.rateUnit} onChange={(e) => set({ rateUnit: e.target.value as RateUnit })}>
                  {(Object.keys(RATE_UNITS) as RateUnit[]).map((u) => (
                    <option key={u} value={u}>
                      {RATE_UNITS[u]}
                    </option>
                  ))}
                </select>
              </label>
              <label className="field">
                <span className="field__label">Hours per day</span>
                <input
                  className="input input--number"
                  type="number"
                  min={1}
                  max={12}
                  placeholder="none"
                  title="Only with this assumption are hourly and daily rates compared"
                  value={criteria.hoursPerDay ?? ''}
                  onChange={(e) => set({ hoursPerDay: num(e.target.value) })}
                />
              </label>
            </div>
            <div className="biz-form-grid">
              <label className="field">
                <span className="field__label">Duration from (months)</span>
                <input
                  className="input input--number"
                  type="number"
                  min={0}
                  value={criteria.durationMinMonths ?? ''}
                  onChange={(e) => set({ durationMinMonths: num(e.target.value) })}
                />
              </label>
              <label className="field">
                <span className="field__label">Duration up to (months)</span>
                <input
                  className="input input--number"
                  type="number"
                  min={0}
                  value={criteria.durationMaxMonths ?? ''}
                  onChange={(e) => set({ durationMaxMonths: num(e.target.value) })}
                />
              </label>
              <label className="field">
                <span className="field__label">Posted within (days)</span>
                <input
                  className="input input--number"
                  type="number"
                  min={1}
                  value={criteria.postedWithinDays ?? ''}
                  onChange={(e) => set({ postedWithinDays: num(e.target.value) })}
                />
              </label>
            </div>
            <div className="biz-card__actions">
              <button
                type="button"
                className="button button--ghost button--small"
                onClick={() => setCriteria(DEFAULT_CONTRACT_CRITERIA)}
              >
                Reset criteria
              </button>
            </div>
          </div>
        )}
      </form>

      {!results && !run.running && (
        <div className="biz-examples" aria-label="Examples">
          {EXAMPLES.map((example) => (
            <button
              key={example}
              type="button"
              className="biz-example"
              onClick={() => {
                setQuery(example);
                setUseCriteria(false);
                search(example, false);
              }}
            >
              {example}
            </button>
          ))}
        </div>
      )}
      {run.error && (
        <p className="form-error" role="alert">
          {run.error}
        </p>
      )}
      {results && <ContractResultsView results={results} onSaved={markSaved} onOpenOpportunity={onOpenOpportunity} />}
    </div>
  );
}

function ContractResultsView({
  results,
  onSaved,
  onOpenOpportunity,
}: {
  results: ContractResults;
  onSaved: (key: string, opportunityId: string) => void;
  onOpenOpportunity: (id: string) => void;
}) {
  const total = results.confirmed.length + results.needsVerification.length + results.notMatching.length;
  return (
    <section className="biz-results" aria-label="Contract results">
      <div className="biz-results__summary">
        <StatusBadge status={results.status} />
        <span className="biz-muted">Retrieved {formatDateTime(results.retrievedAt)}</span>
      </div>
      <Chips items={results.normalized} label="Criteria as ReMa applied them" />
      {total === 0 && results.status !== 'failed' && results.status !== 'offline' && (
        <p className="biz-empty">No advertised engagement in the searched sources matched. ReMa does not fill the gap.</p>
      )}
      {results.confirmed.length === 0 && total > 0 && (
        <p className="biz-empty">
          None meets every hard criterion. The reasons are listed per listing; broaden the criteria yourself if you
          want to.
        </p>
      )}
      <ContractGroup title="Confirmed matches" items={results.confirmed} runId={results.runId} onSaved={onSaved} onOpenOpportunity={onOpenOpportunity} defaultOpen />
      <ContractGroup title="Needs verification" items={results.needsVerification} runId={results.runId} onSaved={onSaved} onOpenOpportunity={onOpenOpportunity} defaultOpen />
      <ContractGroup title="Not matching" items={results.notMatching} runId={results.runId} onSaved={onSaved} onOpenOpportunity={onOpenOpportunity} />
      <SearchedDetails notes={results.notes} sources={results.sources} failures={results.failures} />
    </section>
  );
}

function ContractGroup({
  title,
  items,
  runId,
  onSaved,
  onOpenOpportunity,
  defaultOpen = false,
}: {
  title: string;
  items: ContractResult[];
  runId: string;
  onSaved: (key: string, opportunityId: string) => void;
  onOpenOpportunity: (id: string) => void;
  defaultOpen?: boolean;
}) {
  const [open, setOpen] = useState(defaultOpen);
  const [expanded, setExpanded] = useState<string | null>(null);
  if (items.length === 0) return null;
  return (
    <div className="biz-group">
      <button type="button" className="biz-group__head" aria-expanded={open} onClick={() => setOpen(!open)}>
        {open ? <ChevronDownIcon className="biz-group__chevron" /> : <ChevronRightIcon className="biz-group__chevron" />}
        <span className="biz-group__title">{title}</span>
        <span className="biz-group__count">{items.length}</span>
      </button>
      {open && (
        <div className="table-wrap">
          <table className="data-table biz-table">
            <thead>
              <tr>
                <th scope="col">Project / role</th>
                <th scope="col">Client / agency</th>
                <th scope="col">Location / eligibility</th>
                <th scope="col">Duration / start</th>
                <th scope="col">Rate / budget</th>
                <th scope="col">Match status</th>
                <th scope="col">Verified</th>
                <th scope="col">Source</th>
              </tr>
            </thead>
            <tbody>
              {items.map((c) => {
                const isOpen = expanded === c.key;
                const t = c.terms;
                return (
                  <Fragment key={c.key}>
                    <tr className={isOpen ? 'biz-table__row is-open' : 'biz-table__row'} onClick={() => setExpanded(isOpen ? null : c.key)}>
                      <td className="biz-table__name">
                        <button
                          type="button"
                          className="link-button"
                          aria-expanded={isOpen}
                          onClick={(e) => {
                            e.stopPropagation();
                            setExpanded(isOpen ? null : c.key);
                          }}
                        >
                          {c.title}
                        </button>
                        <span className="biz-muted biz-person__title">{ENGAGEMENT_LABELS[t.engagement]}</span>
                        {c.opportunityId && <span className="badge badge--brand">In Pipeline</span>}
                      </td>
                      <td>
                        {t.agency ? (
                          <>
                            <span>{t.agency}</span>
                            <span className="biz-muted biz-person__title">
                              {t.endClient ? `for ${t.endClient}` : 'End client not disclosed'}
                            </span>
                          </>
                        ) : (
                          (c.company ?? <span className="biz-muted">Not stated</span>)
                        )}
                      </td>
                      <td>
                        {c.location ?? <span className="biz-muted">Not stated</span>}
                        {t.workMode && <span className="biz-muted biz-person__title">{t.workMode}</span>}
                        {t.eligibility.length > 0 && (
                          <span className="biz-muted biz-person__title">{t.eligibility.join('; ')}</span>
                        )}
                      </td>
                      <td>{durationSummary(t)}</td>
                      <td>{rateSummary(t)}</td>
                      <td>
                        <span className={statusClass(c.status)}>{MATCH_LABELS[c.status]}</span>
                      </td>
                      <td>{c.verified}</td>
                      <td>
                        <Links links={[{ label: c.source, url: c.url }]} />
                        {c.postedAt !== null && <span className="biz-muted biz-person__title">posted {formatDate(c.postedAt)}</span>}
                      </td>
                    </tr>
                    {isOpen && (
                      <tr className="biz-table__detail">
                        <td colSpan={8}>
                          <ContractDetail result={c} runId={runId} onSaved={onSaved} onOpenOpportunity={onOpenOpportunity} />
                        </td>
                      </tr>
                    )}
                  </Fragment>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}

export function TermsFacts({ terms: t }: { terms: ContractTerms }) {
  return (
    <dl className="biz-facts">
      <dt>Engagement</dt>
      <dd>{ENGAGEMENT_LABELS[t.engagement]}</dd>
      <dt>Rate / budget</dt>
      <dd>{rateSummary(t)}</dd>
      <dt>Duration</dt>
      <dd>{durationSummary(t)}</dd>
      {t.workload && (
        <>
          <dt>Workload</dt>
          <dd>{t.workload}</dd>
        </>
      )}
      <dt>Work mode</dt>
      <dd>{t.workMode ?? 'Not stated'}</dd>
      <dt>Eligibility</dt>
      <dd>{t.eligibility.length ? t.eligibility.join('; ') : 'Not stated — unknown, not worldwide'}</dd>
      <dt>Agency</dt>
      <dd>{t.agency ?? 'None stated'}</dd>
      <dt>End client</dt>
      <dd>{t.endClient ?? (t.agency ? 'Not disclosed' : 'Not stated')}</dd>
      {t.skills.length > 0 && (
        <>
          <dt>Skills</dt>
          <dd>{t.skills.join(', ')}</dd>
        </>
      )}
      {t.languages.length > 0 && (
        <>
          <dt>Languages</dt>
          <dd>{t.languages.join(', ')}</dd>
        </>
      )}
    </dl>
  );
}

function ContractDetail({
  result: c,
  runId,
  onSaved,
  onOpenOpportunity,
}: {
  result: ContractResult;
  runId: string;
  onSaved: (key: string, opportunityId: string) => void;
  onOpenOpportunity: (id: string) => void;
}) {
  const [saved, setSaved] = useState<string | null>(null);
  const action = useAction();
  return (
    <div className="biz-detail">
      <div className="biz-detail__cols">
        <div>
          <h4 className="biz-detail__title">Terms as stated</h4>
          <TermsFacts terms={c.terms} />
        </div>
        <div>
          <h4 className="biz-detail__title">Why this status</h4>
          <ul className="biz-reasons">
            {c.reasons.map((r) => (
              <li key={r}>{r}</li>
            ))}
            {c.reasons.length === 0 && (
              <li>
                {c.status === 'confirmed'
                  ? 'The listing’s own terms meet every hard criterion.'
                  : 'The listing does not state enough to decide.'}
              </li>
            )}
          </ul>
          {c.scope && (
            <>
              <h4 className="biz-detail__title">Scope</h4>
              <p className="biz-scope">{c.scope}</p>
            </>
          )}
        </div>
      </div>
      <p className="biz-footnote">
        Opening a listing is not applying, contacting anyone or agreeing to terms. Labels are the listing’s own words,
        not a legal assessment of employment status.
      </p>
      <div className="biz-save">
        <Links links={[{ label: 'Open source', url: c.url }]} />
        {c.opportunityId ? (
          <button type="button" className="button button--secondary button--small" onClick={() => onOpenOpportunity(c.opportunityId ?? '')}>
            Open in Pipeline
          </button>
        ) : (
          <button
            type="button"
            className="button button--primary button--small"
            disabled={action.busy}
            onClick={() =>
              void action.run(async () => {
                const result = await saveContract(runId, c.key);
                onSaved(c.key, result.opportunity.id);
                setSaved(result.created ? 'Saved as a New Lead (no Application was created).' : 'Already in your Pipeline.');
              })
            }
          >
            Save to Pipeline
          </button>
        )}
        {saved && (
          <span className="biz-saved" role="status">
            {saved}
          </span>
        )}
        {action.error && (
          <span className="form-error" role="alert">
            {action.error}
          </span>
        )}
      </div>
    </div>
  );
}
