import { Fragment, useState } from 'react';

import { formatDateTime, formatPercent, jobsText, RESOURCE_TYPES } from '../../lib/analytics';
import type { LearningCriteria, LearningRecommendation, LearningView } from '../../services/analyticsService';
import { ChevronDownIcon, ChevronRightIcon, SparkleIcon } from '../icons';
import { Notes, PriorityBadge, StateBadge, WebLink } from './common';

function Criteria({ criteria, onChange }: { criteria: LearningCriteria; onChange: (c: LearningCriteria) => void }) {
  return (
    <div className="a-criteria-form" role="group" aria-label="Learning criteria">
      <label className="a-inline">
        Required by at least
        <input
          className="input a-number"
          type="number"
          min={0}
          max={100}
          step={5}
          value={criteria.minPercent ?? 0}
          onChange={(e) => onChange({ ...criteria, minPercent: Math.min(100, Math.max(0, Number(e.target.value) || 0)) })}
        />
        % of the jobs
      </label>
      <label className="a-inline">
        At most
        <input
          className="input a-number"
          type="number"
          min={1}
          max={15}
          value={criteria.maxGaps}
          onChange={(e) => onChange({ ...criteria, maxGaps: Math.min(15, Math.max(1, Number(e.target.value) || 1)) })}
        />
        gaps
      </label>
      <label className="a-check">
        <input type="checkbox" checked={criteria.ignoreSingle} onChange={(e) => onChange({ ...criteria, ignoreSingle: e.target.checked })} />
        Ignore skills that appear only once
      </label>
      <label className="a-check">
        <input
          type="checkbox"
          checked={criteria.includePartial}
          onChange={(e) => onChange({ ...criteria, includePartial: e.target.checked })}
        />
        Include partly covered skills
      </label>
      {(criteria.focus ?? []).length > 0 && (
        <span className="a-values">
          Focus:
          {(criteria.focus ?? []).map((f) => (
            <span key={f} className="a-chip">
              {f}
              <button
                type="button"
                aria-label={`Remove ${f}`}
                onClick={() => onChange({ ...criteria, focus: (criteria.focus ?? []).filter((x) => x !== f) })}
              >
                ×
              </button>
            </span>
          ))}
        </span>
      )}
    </div>
  );
}

function Recommendation({ rec }: { rec: LearningRecommendation }) {
  return (
    <div className="rec">
      <div className="rec__col">
        <h4>Why it matters for these jobs</h4>
        <ul className="rec__why">
          {rec.why.map((w) => (
            <li key={w}>{w}</li>
          ))}
        </ul>
        {rec.path.length > 0 && (
          <>
            <h4>Recommended path</h4>
            <ol className="rec__path">
              {rec.path.map((step) => (
                <li key={step}>{step}</li>
              ))}
            </ol>
          </>
        )}
      </div>
      <div className="rec__col">
        <h4>Resources</h4>
        <ul className="rec__resources">
          {rec.resources.map((r) => (
            <li key={r.url}>
              <WebLink url={r.url}>{r.title}</WebLink>
              <span className="a-muted">
                {' '}
                · {RESOURCE_TYPES[r.kind]}
                {r.provider && ` · ${r.provider}`}
                {r.level && ` · ${r.level}`}
                {r.cost && ` · ${r.cost}`}
              </span>
              {r.note && <p className="rec__note">{r.note}</p>}
              {r.reachable === false && (
                <p className="rec__warning">ReMa could not open this link when it checked; verify it before relying on it.</p>
              )}
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}

export function LearningTab({
  view,
  criteria,
  onCriteria,
  onResearch,
  researching,
  error,
  onOpenProfile,
}: {
  view: LearningView;
  criteria: LearningCriteria;
  onCriteria: (c: LearningCriteria) => void;
  onResearch: () => void;
  researching: boolean;
  error: string | null;
  onOpenProfile: () => void;
}) {
  const [open, setOpen] = useState<string | null>(null);
  const research = view.research;
  const running = researching || research?.status === 'running';
  const current = research?.current ? research : null;
  const demand = (skill: string) => view.gaps.find((g) => g.name === skill) ?? research?.gaps.find((g) => g.name === skill);

  return (
    <div className="a-stack">
      <Criteria criteria={criteria} onChange={onCriteria} />
      {!view.profileAvailable && (
        <div className="a-callout">
          <p>
            <strong>Profile required.</strong> Learning recommendations start from the gaps between the selected jobs
            and your Profile.
          </p>
          <button type="button" className="button button--secondary" onClick={onOpenProfile}>
            Open Profile
          </button>
        </div>
      )}
      <Notes notes={view.notes} />

      {view.gaps.length > 0 && (
        <section className="viz-card">
          <header className="viz-card__head">
            <div>
              <h3 className="viz-card__title">Prioritized gaps</h3>
              <p className="viz-card__subtitle">
                Calculated from the {jobsText(view.jobsWithRequirements)} with requirement data in this analysis — this is what ReMa
                researches.
              </p>
            </div>
          </header>
          <table className="a-table a-table--compact">
            <thead>
              <tr>
                <th>Skill gap</th>
                <th className="a-num">Demand</th>
                <th>Priority</th>
                <th>Profile</th>
                <th>Why</th>
              </tr>
            </thead>
            <tbody>
              {view.gaps.map((g) => (
                <tr key={g.name}>
                  <td>{g.name}</td>
                  <td className="a-num">{formatPercent(g.percent)}</td>
                  <td>
                    <PriorityBadge level={g.level} />
                  </td>
                  <td>
                    <StateBadge state={g.state} />
                  </td>
                  <td className="a-muted">{g.reasons[0]}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      )}

      <div className="a-research">
        <button
          type="button"
          className="button button--primary"
          disabled={running || view.gaps.length === 0 || !view.model}
          onClick={onResearch}
        >
          <SparkleIcon className="button__icon" />
          {running ? 'Researching…' : current ? 'Refresh recommendations' : 'Research learning resources'}
        </button>
        <span className="a-muted">
          {!view.model
            ? 'Choose a default model in Settings to research resources.'
            : running
              ? `${view.model} is looking for resources for these gaps. You can keep working.`
              : `Uses ${view.model}. Results are stored; opening Analytics never re-runs research.`}
        </span>
      </div>
      {error && <p className="form-error">{error}</p>}

      {research && research.status !== 'running' && (
        <section className="viz-card">
          <header className="viz-card__head">
            <div>
              <h3 className="viz-card__title">Recommended resources</h3>
              <p className="viz-card__subtitle">
                Generated {formatDateTime(research.generatedAt)} for “{research.datasetLabel}” ({jobsText(research.jobCount)})
                {research.model && ` · ${research.model}`}
              </p>
            </div>
          </header>
          {!research.current && (
            <p className="a-callout a-callout--quiet">
              These recommendations were researched for a different selection or criteria. Research again to match the
              current jobs.
            </p>
          )}
          {research.stale && (
            <p className="a-callout a-callout--quiet">The gaps of this selection changed since this research. Refresh to update it.</p>
          )}
          {research.status === 'failed' ? (
            <p className="form-error">Research failed: {research.error ?? 'unknown error'}</p>
          ) : (
            <table className="a-table a-table--compact">
              <thead>
                <tr>
                  <th aria-label="Details" />
                  <th>Skill gap</th>
                  <th className="a-num">Demand</th>
                  <th>Priority</th>
                  <th>Recommended resource</th>
                  <th>Type</th>
                </tr>
              </thead>
              <tbody>
                {research.recommendations.map((rec) => {
                  const first = rec.resources[0];
                  const expanded = open === rec.skill;
                  return (
                    <Fragment key={rec.skill}>
                      <tr className="a-row--link" onClick={() => setOpen(expanded ? null : rec.skill)}>
                        <td>{expanded ? <ChevronDownIcon className="a-row__chevron" /> : <ChevronRightIcon className="a-row__chevron" />}</td>
                        <td>{rec.skill}</td>
                        <td className="a-num">{formatPercent(demand(rec.skill)?.percent)}</td>
                        <td>
                          <PriorityBadge level={rec.level} />
                        </td>
                        <td onClick={(e) => e.stopPropagation()}>{first && <WebLink url={first.url}>{first.title}</WebLink>}</td>
                        <td>{first && RESOURCE_TYPES[first.kind]}</td>
                      </tr>
                      {expanded && (
                        <tr className="a-row--detail">
                          <td />
                          <td colSpan={5}>
                            <Recommendation rec={rec} />
                          </td>
                        </tr>
                      )}
                    </Fragment>
                  );
                })}
              </tbody>
            </table>
          )}
        </section>
      )}
    </div>
  );
}
