import { COLUMNS, formatDay, formatPercent, SENIORITIES, WORK_MODES } from '../../lib/analytics';
import type { AnalyticsOverview, JobColumn, RankedJob } from '../../services/analyticsService';
import { openExternalUrl } from '../../services/systemService';
import { ChartIcon, ExternalIcon } from '../icons';
import { IconButton } from '../ui/IconButton';
import { useOpenLink } from '../../hooks/useOpenLink';

const ALL_COLUMNS = Object.keys(COLUMNS) as JobColumn[];

function Missing() {
  return (
    <span className="a-muted" title="Not stated">
      —
    </span>
  );
}

function cell(job: RankedJob, column: JobColumn) {
  switch (column) {
    case 'rank':
      return job.rank;
    case 'company':
      return job.company ?? <Missing />;
    case 'role':
      return (
        <>
          <span className="a-job__title">
            {job.title}
            {job.details === 'pending' && (
              <span className="a-pending" title="ReMa is still reading this job's details">
                reading…
              </span>
            )}
          </span>
          {/* In a narrow panel, company and location sit under the title. */}
          <span className="a-job__meta">{[job.company, job.location].filter(Boolean).join(' · ')}</span>
        </>
      );
    case 'location':
      return job.location ?? <Missing />;
    case 'work_mode':
      return job.workMode ? WORK_MODES[job.workMode] : <Missing />;
    case 'salary': {
      if (!job.salary) return <Missing />;
      // "€95k–115k / year": the amount stays on one line; the period may wrap.
      const [amount, period] = job.salary.split(' / ');
      return (
        <span
          className={job.salaryComparable ? undefined : 'a-muted'}
          title={job.salaryComparable ? undefined : 'Not compared: other currency or period'}
        >
          <span className="a-nowrap">{amount}</span>
          {period && <span className="a-salary__period"> / {period}</span>}
        </span>
      );
    }
    case 'seniority':
      return job.seniority ? SENIORITIES[job.seniority] : <Missing />;
    case 'match':
      return job.matchPercent == null ? <Missing /> : formatPercent(job.matchPercent);
    case 'skill_gap':
      return job.missing == null ? <Missing /> : job.missing;
    case 'posted':
      return job.datePosted == null ? <Missing /> : formatDay(job.datePosted);
    case 'discovered':
      return formatDay(job.dateDiscovered);
    case 'source':
      return job.source ?? <Missing />;
  }
}

const NUMERIC: JobColumn[] = ['rank', 'match', 'skill_gap'];

export function JobsTab({
  overview,
  columns,
  onColumns,
  selected,
  onSelected,
  onAnalyzeJobs,
}: {
  overview: AnalyticsOverview;
  columns: JobColumn[];
  onColumns: (columns: JobColumn[]) => void;
  selected: number[];
  onSelected: (ids: number[]) => void;
  onAnalyzeJobs: (ids: number[]) => void;
}) {
  const open = useOpenLink();
  const jobs = overview.jobs;
  const allSelected = jobs.length > 0 && jobs.every((j) => selected.includes(j.id));
  const toggle = (id: number) => onSelected(selected.includes(id) ? selected.filter((s) => s !== id) : [...selected, id]);
  const shown = ALL_COLUMNS.filter((c) => columns.includes(c));

  return (
    <div className="a-jobs">
      <div className="a-toolbar">
        <span className="a-muted">
          {overview.summary.analyzed} job{overview.summary.analyzed === 1 ? '' : 's'}
          {overview.more > 0 && ` · showing the first ${jobs.length}`}
        </span>
        {selected.length > 0 && (
          <>
            <button type="button" className="button button--secondary" onClick={() => onAnalyzeJobs(selected)}>
              <ChartIcon className="button__icon" />
              Analyze {selected.length} selected
            </button>
            <button type="button" className="link-button" onClick={() => onSelected([])}>
              Clear selection
            </button>
          </>
        )}
        <details className="a-menu">
          <summary className="button button--ghost">Columns</summary>
          <div className="a-menu__body">
            {ALL_COLUMNS.map((c) => (
              <label key={c} className="a-check">
                <input
                  type="checkbox"
                  checked={columns.includes(c)}
                  disabled={c === 'role'}
                  onChange={() => onColumns(columns.includes(c) ? columns.filter((x) => x !== c) : [...columns, c])}
                />
                {COLUMNS[c]}
              </label>
            ))}
          </div>
        </details>
      </div>
      {jobs.length === 0 ? (
        <p className="a-empty">No jobs match the current scope and filters.</p>
      ) : (
        <div className="a-table-wrap">
          <table className="a-table">
            <thead>
              <tr>
                <th className="a-table__pick">
                  <input
                    type="checkbox"
                    aria-label="Select all jobs"
                    checked={allSelected}
                    onChange={() => onSelected(allSelected ? [] : jobs.map((j) => j.id))}
                  />
                </th>
                {shown.map((c) => (
                  <th key={c} className={`a-col--${c}${NUMERIC.includes(c) ? ' a-num' : ''}`}>
                    {COLUMNS[c]}
                  </th>
                ))}
                <th aria-label="Actions" />
              </tr>
            </thead>
            <tbody>
              {jobs.map((job) => (
                <tr
                  key={job.id}
                  className={`a-row${job.url ? ' a-row--link' : ''}${selected.includes(job.id) ? ' a-row--selected' : ''}`}
                  title={job.url ? 'Open the job in the ReMa browser' : 'No link stored for this job'}
                  onClick={(e) => job.url && open(job.url, e)}
                >
                  <td className="a-table__pick" onClick={(e) => e.stopPropagation()}>
                    <input
                      type="checkbox"
                      aria-label={`Select ${job.title}`}
                      checked={selected.includes(job.id)}
                      onChange={() => toggle(job.id)}
                    />
                  </td>
                  {shown.map((c) => (
                    <td key={c} className={`a-col--${c}${NUMERIC.includes(c) ? ' a-num' : ''}`}>
                      {cell(job, c)}
                    </td>
                  ))}
                  <td className="a-table__actions" onClick={(e) => e.stopPropagation()}>
                    <IconButton label="Analyze only this job" className="icon-button--small" onClick={() => onAnalyzeJobs([job.id])}>
                      <ChartIcon />
                    </IconButton>
                    {job.url && (
                      <IconButton
                        label="Open in external browser"
                        className="icon-button--small"
                        onClick={() => void openExternalUrl(job.url ?? '').catch(() => {})}
                      >
                        <ExternalIcon />
                      </IconButton>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
