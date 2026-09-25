import { CATEGORIES, CLASSES, formatPercent, jobsText, STATES } from '../../lib/analytics';
import type {
  FrequencyClass,
  MatchState,
  RequirementCategory,
  RequirementSort,
  RequirementTableQuery,
  RequirementsView,
} from '../../services/analyticsService';
import { Notes, StateBadge, Summary } from './common';

function SortHeader({
  label,
  sort,
  table,
  onTable,
  numeric,
}: {
  label: string;
  sort: RequirementSort;
  table: RequirementTableQuery;
  onTable: (t: RequirementTableQuery) => void;
  numeric?: boolean;
}) {
  const active = table.sort === sort;
  const next = active ? (table.direction === 'desc' ? 'asc' : 'desc') : sort === 'jobs' ? 'desc' : 'asc';
  return (
    <th className={numeric ? 'a-num' : undefined} aria-sort={active ? (table.direction === 'desc' ? 'descending' : 'ascending') : 'none'}>
      <button type="button" className="a-sort" onClick={() => onTable({ ...table, sort, direction: next })}>
        {label}
        {active && <span aria-hidden="true">{table.direction === 'desc' ? ' ↓' : ' ↑'}</span>}
      </button>
    </th>
  );
}

export function RequirementsTab({
  view,
  table,
  onTable,
  selected,
  onSelected,
  onFilterJobs,
  onFocusLearning,
}: {
  view: RequirementsView;
  table: RequirementTableQuery;
  onTable: (t: RequirementTableQuery) => void;
  selected: string[];
  onSelected: (names: string[]) => void;
  onFilterJobs: (names: string[]) => void;
  onFocusLearning: (names: string[]) => void;
}) {
  const toggleClass = (c: FrequencyClass) =>
    onTable({ ...table, classes: table.classes.includes(c) ? table.classes.filter((x) => x !== c) : [...table.classes, c] });
  const toggle = (name: string) =>
    onSelected(selected.includes(name) ? selected.filter((s) => s !== name) : [...selected, name]);
  const filtered =
    table.search.trim() !== '' || table.categories.length > 0 || table.classes.length > 0 || table.states.length > 0;

  return (
    <div className="a-stack">
      <Summary lines={view.summary} />
      <Notes
        notes={
          view.jobsWithRequirements < view.jobs
            ? [`Frequencies use the ${jobsText(view.jobsWithRequirements)} with requirement data.`]
            : []
        }
      />

      <div className="a-classes" role="group" aria-label="Frequency classes">
        {view.classes.map((c) => (
          <button
            key={c.class}
            type="button"
            className={`a-class${table.classes.includes(c.class) ? ' a-class--on' : ''}`}
            onClick={() => toggleClass(c.class)}
            aria-pressed={table.classes.includes(c.class)}
          >
            <span className="a-class__count">{c.requirements}</span>
            <span className="a-class__name">{CLASSES[c.class]}</span>
            <span className="a-muted">{c.class === 'rare' ? '< 15%' : `≥ ${c.threshold}%`}</span>
          </button>
        ))}
      </div>

      <div className="a-toolbar">
        <input
          className="input a-search"
          type="search"
          placeholder="Search requirements…"
          aria-label="Search requirements"
          value={table.search}
          onChange={(e) => onTable({ ...table, search: e.target.value })}
        />
        <select
          className="input input--auto"
          aria-label="Category"
          value={table.categories[0] ?? ''}
          onChange={(e) =>
            onTable({ ...table, categories: e.target.value ? [e.target.value as RequirementCategory] : [] })
          }
        >
          <option value="">All categories</option>
          {view.categories.map((c) => (
            <option key={c.category} value={c.category}>
              {CATEGORIES[c.category]} ({c.requirements})
            </option>
          ))}
        </select>
        {view.profileAvailable && (
          <select
            className="input input--auto"
            aria-label="Profile state"
            value={table.states[0] ?? ''}
            onChange={(e) => onTable({ ...table, states: e.target.value ? [e.target.value as MatchState] : [] })}
          >
            <option value="">Any Profile state</option>
            {(Object.keys(STATES) as MatchState[]).map((s) => (
              <option key={s} value={s}>
                {STATES[s]}
              </option>
            ))}
          </select>
        )}
        {filtered && (
          <button
            type="button"
            className="link-button"
            onClick={() => onTable({ ...table, search: '', categories: [], classes: [], states: [] })}
          >
            Show all
          </button>
        )}
      </div>

      {selected.length > 0 && (
        <div className="a-selection" role="region" aria-label="Selected requirements">
          <span>
            {selected.length} selected: {selected.join(', ')}
          </span>
          <button type="button" className="button button--secondary" onClick={() => onFilterJobs(selected)}>
            Show jobs requiring {selected.length === 1 ? 'it' : 'any of them'}
          </button>
          {view.profileAvailable && (
            <button type="button" className="button button--secondary" onClick={() => onFocusLearning(selected)}>
              Focus learning on {selected.length === 1 ? 'it' : 'them'}
            </button>
          )}
          <button type="button" className="link-button" onClick={() => onSelected([])}>
            Clear
          </button>
        </div>
      )}

      <p className="a-muted">
        {view.rows.length} of {view.total} distinct requirements
      </p>
      <div className="a-table-wrap">
        <table className="a-table">
          <thead>
            <tr>
              <th className="a-table__pick" aria-label="Select" />
              <SortHeader label="Requirement" sort="name" table={table} onTable={onTable} />
              <SortHeader label="Category" sort="category" table={table} onTable={onTable} />
              <SortHeader label="Jobs" sort="jobs" table={table} onTable={onTable} numeric />
              <th className="a-num">Frequency</th>
              <th>Class</th>
              {view.profileAvailable && <SortHeader label="Profile" sort="state" table={table} onTable={onTable} />}
            </tr>
          </thead>
          <tbody>
            {view.rows.map((r) => (
              <tr key={`${r.kind}-${r.name}`} className={selected.includes(r.name) ? 'a-row--selected' : undefined}>
                <td className="a-table__pick">
                  <input type="checkbox" aria-label={`Select ${r.name}`} checked={selected.includes(r.name)} onChange={() => toggle(r.name)} />
                </td>
                <td title={r.examples.length ? `As written: ${r.examples.join(' · ')}` : undefined}>
                  {r.name}
                  {r.level && <span className="a-muted"> · {r.level}</span>}
                  {r.preferred > 0 && (
                    <span className="a-muted" title={`Required by ${r.required}, preferred by ${r.preferred}`}>
                      {' '}
                      · {r.required === 0 ? 'preferred' : `${r.preferred} preferred`}
                    </span>
                  )}
                </td>
                <td>{CATEGORIES[r.category]}</td>
                <td className="a-num">{r.jobs}</td>
                <td className="a-num">
                  <span className="freq">
                    <span className="freq__bar" aria-hidden="true">
                      <span style={{ width: `${r.percent ?? 0}%` }} />
                    </span>
                    {formatPercent(r.percent)}
                  </span>
                </td>
                <td>{CLASSES[r.class]}</td>
                {view.profileAvailable && (
                  <td>
                    <StateBadge state={r.state} />
                  </td>
                )}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}
