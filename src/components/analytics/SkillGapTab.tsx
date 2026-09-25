import { useMemo } from 'react';

import { CATEGORIES, formatPercent, jobsText, PRIORITIES, STATE_ICONS, STATES } from '../../lib/analytics';
import type {
  CellState,
  GapPriority,
  MatchState,
  SkillDemand,
  SkillGapOptions,
  SkillGapView,
} from '../../services/analyticsService';
import { BarChart, ChartCard, type BarRow } from './Charts';
import { Notes, PriorityBadge, Stat, StateBadge, Summary } from './common';

const DEMAND_ROWS = 15;
const PRIORITY_ROWS = 10;
const LANE_ITEMS = 6;

const CELL_TEXT: Record<CellState, string> = {
  matched: 'Matched',
  partial: 'Partial',
  missing: 'Missing',
  unknown: 'Unknown',
  not_required: 'Not required',
  no_data: 'No requirement data',
};

const CELL_ICON: Record<CellState, string> = {
  ...STATE_ICONS,
  not_required: '',
  no_data: '·',
};

function DemandTable({ rows }: { rows: SkillDemand[] }) {
  return (
    <table className="a-table a-table--compact">
      <thead>
        <tr>
          <th>Skill</th>
          <th>Category</th>
          <th className="a-num">Jobs</th>
          <th className="a-num">Share</th>
          <th>Profile</th>
        </tr>
      </thead>
      <tbody>
        {rows.map((d) => (
          <tr key={d.name}>
            <td>{d.name}</td>
            <td>{CATEGORIES[d.category]}</td>
            <td className="a-num">{d.jobs}</td>
            <td className="a-num">{formatPercent(d.percent)}</td>
            <td>
              <StateBadge state={d.state} title={d.evidence} />
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

/** Viz 2: how much of the market's skill demand the Profile covers. */
function CoverageVsDemand({ view }: { view: SkillGapView }) {
  const lanes = useMemo(() => {
    const by: Record<MatchState, SkillDemand[]> = { matched: [], partial: [], missing: [], unknown: [] };
    for (const d of view.demand) if (d.state) by[d.state].push(d);
    return by;
  }, [view.demand]);
  const shares = view.coverage.filter((s) => s.mentions > 0);
  return (
    <ChartCard
      title="Profile coverage vs market demand"
      subtitle="Share of all requirement mentions in the selected jobs, by what your Profile shows."
      table={<DemandTable rows={view.demand} />}
    >
      <div className="share-bar" role="img" aria-label={shares.map((s) => `${STATES[s.state]} ${formatPercent(s.percent)}`).join(', ')}>
        {shares.map((s) => (
          <span
            key={s.state}
            className={`share-bar__part state-fill--${s.state}`}
            style={{ flexGrow: s.percent ?? 0 }}
            title={`${STATES[s.state]}: ${s.mentions} mentions (${formatPercent(s.percent)}), ${s.requirements} requirements`}
          />
        ))}
      </div>
      <ul className="share-legend">
        {view.coverage.map((s) => (
          <li key={s.state}>
            <StateBadge state={s.state} />
            <strong>{formatPercent(s.percent)}</strong>
            <span className="a-muted">
              {s.requirements} requirement{s.requirements === 1 ? '' : 's'}
            </span>
          </li>
        ))}
      </ul>
      <div className="lanes">
        {(['matched', 'partial', 'missing'] as MatchState[]).map((state) => (
          <div key={state} className="lane">
            <h4 className="lane__title">
              <StateBadge state={state} />
              <span className="a-muted">
                {state === 'matched' ? 'you have' : state === 'partial' ? 'related skills only' : 'by demand'}
              </span>
            </h4>
            {lanes[state].length === 0 ? (
              <p className="a-muted">None among the most requested skills.</p>
            ) : (
              <ul className="lane__items">
                {lanes[state].slice(0, LANE_ITEMS).map((d) => (
                  <li key={d.name} title={d.evidence ?? undefined}>
                    <span className="lane__name">{d.name}</span>
                    <span className="lane__bar" aria-hidden="true">
                      <span className={`state-fill--${state}`} style={{ width: `${d.percent ?? 0}%` }} />
                    </span>
                    <span className="lane__value">{formatPercent(d.percent)}</span>
                  </li>
                ))}
              </ul>
            )}
          </div>
        ))}
      </div>
    </ChartCard>
  );
}

/** Viz 3: which jobs a missing skill affects. */
function GapMatrix({ view, onJob }: { view: SkillGapView; onJob: (id: number) => void }) {
  const m = view.matrix;
  if (m.skills.length === 0 || m.rows.length === 0) return null;
  return (
    <ChartCard
      title="Job × skill gap matrix"
      subtitle={
        m.moreRows > 0
          ? `The ${m.rows.length} top-ranked jobs; the “gap in” row counts all ${view.jobs} jobs.`
          : 'The most requested skills in each selected job.'
      }
    >
      <div className="matrix-wrap">
        <table className="matrix">
          <thead>
            <tr>
              <th className="matrix__job">Job</th>
              {m.skills.map((s) => (
                <th key={s} className="matrix__skill">
                  <span>{s}</span>
                </th>
              ))}
              <th className="a-num">Match</th>
            </tr>
          </thead>
          <tbody>
            {m.rows.map((row) => (
              <tr key={row.jobId}>
                <th className="matrix__job" scope="row">
                  <button type="button" className="link-button" title="Analyze only this job" onClick={() => onJob(row.jobId)}>
                    {row.title}
                  </button>
                  {row.company && <span className="a-muted"> · {row.company}</span>}
                </th>
                {row.cells.map((c, i) => (
                  <td
                    key={m.skills[i]}
                    className={`matrix__cell matrix__cell--${c}`}
                    title={`${m.skills[i]}: ${CELL_TEXT[c]}`}
                    aria-label={`${m.skills[i]}: ${CELL_TEXT[c]}`}
                  >
                    {CELL_ICON[c]}
                  </td>
                ))}
                <td className="a-num">{formatPercent(row.coverage)}</td>
              </tr>
            ))}
          </tbody>
          {view.profileAvailable && (
            <tfoot>
              <tr>
                <th className="matrix__job" scope="row">
                  Gap in (jobs)
                </th>
                {m.gapJobs.map((n, i) => (
                  <td key={m.skills[i]} className="a-num">
                    {n}
                  </td>
                ))}
                <td />
              </tr>
            </tfoot>
          )}
        </table>
      </div>
      <ul className="matrix-legend">
        {(['matched', 'partial', 'missing', 'unknown', 'not_required'] as CellState[]).map((c) => (
          <li key={c}>
            <span className={`matrix__cell matrix__cell--${c} matrix-legend__key`}>{CELL_ICON[c]}</span>
            {CELL_TEXT[c]}
          </li>
        ))}
      </ul>
    </ChartCard>
  );
}

function PriorityList({ priorities }: { priorities: GapPriority[] }) {
  return (
    <ol className="priority-list">
      {priorities.map((p) => (
        <li key={p.name}>
          <div className="priority-list__head">
            <strong>{p.name}</strong>
            <PriorityBadge level={p.level} />
            <StateBadge state={p.state} />
            <span className="a-muted">score {(p.score ?? 0).toFixed(2)}</span>
          </div>
          <ul className="priority-list__reasons">
            {p.reasons.map((r) => (
              <li key={r}>{r}</li>
            ))}
          </ul>
        </li>
      ))}
    </ol>
  );
}

export function SkillGapTab({
  view,
  options,
  onOptions,
  onJob,
  onOpenProfile,
}: {
  view: SkillGapView;
  options: SkillGapOptions;
  onOptions: (options: SkillGapOptions) => void;
  onJob: (jobId: number) => void;
  onOpenProfile: () => void;
}) {
  const demandRows: BarRow[] = view.demand.slice(0, DEMAND_ROWS).map((d) => ({
    label: d.name,
    value: d.percent ?? 0,
    display: `${formatPercent(d.percent)} · ${d.jobs}`,
    detail: [
      `${d.jobs} of ${jobsText(view.jobsWithRequirements)} (${formatPercent(d.percent)})`,
      CATEGORIES[d.category],
      ...(d.state ? [`Profile: ${STATES[d.state]}${d.evidence ? ` (${d.evidence})` : ''}`] : []),
    ],
  }));
  const top = view.priorities.slice(0, PRIORITY_ROWS);
  const priorityRows: BarRow[] = top.map((p) => ({
    label: p.name,
    value: p.score ?? 0,
    display: `${(p.score ?? 0).toFixed(2)} · ${PRIORITIES[p.level]}`,
    detail: p.reasons,
  }));
  const maxScore = Math.max(0.5, ...top.map((p) => p.score ?? 0));

  return (
    <div className="a-stack">
      <label className="a-check">
        <input type="checkbox" checked={options.weightByRank} onChange={(e) => onOptions({ weightByRank: e.target.checked })} />
        Weight by my ranking (higher-ranked jobs count up to twice as much)
      </label>

      {!view.profileAvailable && (
        <div className="a-callout">
          <p>
            <strong>Profile required for personal skill-gap comparison.</strong> Market demand is shown below; add your
            skills to compare them.
          </p>
          <button type="button" className="button button--secondary" onClick={onOpenProfile}>
            Open Profile
          </button>
        </div>
      )}

      <div className="a-stats">
        <Stat label="Selected jobs" value={view.jobs} />
        <Stat label="With requirement data" value={view.jobsWithRequirements} hint="The others count as unknown, not as 'requires nothing'." />
        {view.profileAvailable && (
          <Stat
            label="Average skill coverage"
            value={formatPercent(view.averageCoverage)}
            hint="(matched + ½ partial) ÷ (matched + partial + missing); unknown requirements are left out."
          />
        )}
        {view.profileAvailable && <Stat label="Missing requirements" value={view.counts.missing} hint="Summed over the selected jobs." />}
      </div>

      <Summary lines={view.summary} />
      <Notes notes={view.notes} />

      {view.job && (
        <section className="viz-card">
          <header className="viz-card__head">
            <div>
              <h3 className="viz-card__title">
                {view.job.title}
                {view.job.company && <span className="a-muted"> · {view.job.company}</span>}
              </h3>
              <p className="viz-card__subtitle">
                {view.job.counts.requirements} requirements · {view.job.counts.matched} matched · {view.job.counts.partial}{' '}
                partial · {view.job.counts.missing} missing · {view.job.counts.unknown} unknown
                {view.job.coverage != null && ` · coverage ${formatPercent(view.job.coverage)}`}
              </p>
            </div>
          </header>
          <table className="a-table a-table--compact">
            <thead>
              <tr>
                <th>Requirement</th>
                <th>Category</th>
                <th>Importance</th>
                <th>Profile</th>
              </tr>
            </thead>
            <tbody>
              {view.job.requirements.map((r) => (
                <tr key={`${r.kind}-${r.name}`} title={r.original}>
                  <td>{r.name}</td>
                  <td>{CATEGORIES[r.category]}</td>
                  <td>{r.importance === 'required' ? 'Required' : 'Preferred'}</td>
                  <td>
                    <StateBadge state={r.state} title={r.evidence} />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      )}

      {demandRows.length > 0 ? (
        <ChartCard
          title="Skill demand frequency"
          subtitle={`Share of the ${jobsText(view.jobsWithRequirements)} with requirement data that ask for each skill.`}
          table={<DemandTable rows={view.demand} />}
        >
          <BarChart rows={demandRows} max={100} ariaLabel="Skill demand frequency" tick={(v) => `${v}%`} />
        </ChartCard>
      ) : (
        <p className="a-empty">No requirement data yet for these jobs. Details are read in the background.</p>
      )}

      {view.profileAvailable && view.demand.length > 0 && <CoverageVsDemand view={view} />}

      <GapMatrix view={view} onJob={onJob} />

      {view.profileAvailable && top.length > 0 && (
        <ChartCard
          title="Gap priority"
          subtitle="Frequency among the selected jobs × required (1) or preferred (½) × missing (1) or partial (½), lifted when the gap is more common among the better-paid jobs."
          table={<PriorityList priorities={view.priorities} />}
        >
          <BarChart
            rows={priorityRows}
            max={maxScore}
            ariaLabel="Gap priority"
            thresholds={[
              { value: 0.2, label: 'Medium' },
              { value: 0.4, label: 'High' },
            ]}
            tick={(v) => v.toFixed(1)}
          />
          <PriorityList priorities={top.slice(0, 5)} />
        </ChartCard>
      )}
    </div>
  );
}
