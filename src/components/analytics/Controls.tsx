import { useId, useState } from 'react';

import {
  ALL_SCOPE,
  conditionReady,
  DEFAULT_RANKING,
  describeCondition,
  directionLabel,
  emptyCondition,
  ENUMS,
  FIELD_OPS,
  FIELDS,
  formatDateTime,
  jobsText,
  OPS,
  PREFER_KEYS,
  RANKING_KEYS,
} from '../../lib/analytics';
import type {
  DataScope,
  Facets,
  FacetValue,
  FilterCondition,
  FilterField,
  FilterOp,
  JobSearchRun,
  SortCriterion,
  SortKey,
} from '../../services/analyticsService';
import { ChevronDownIcon, ChevronUpIcon, CloseIcon, PlusIcon, TrashIcon } from '../icons';
import { IconButton } from '../ui/IconButton';

const SOURCES: Record<string, string> = { chat: 'Chat', task: 'Scheduled task', manual: 'Added', tool: 'Search' };

// ── Data scope ────────────────────────────────────────────────────────

export function ScopeEditor({
  scope,
  runs,
  selectedJobs,
  onChange,
  onDeleteRun,
}: {
  scope: DataScope;
  runs: JobSearchRun[];
  selectedJobs: number[];
  onChange: (scope: DataScope) => void;
  onDeleteRun: (run: JobSearchRun) => void;
}) {
  const name = useId();
  const [confirming, setConfirming] = useState<number | null>(null);
  const toggleRun = (id: number) => {
    const ids = scope.runIds.includes(id) ? scope.runIds.filter((r) => r !== id) : [...scope.runIds, id];
    onChange({ kind: 'searches', runIds: ids, jobIds: [] });
  };
  const total = runs.reduce((n, r) => n + r.resultCount, 0);
  return (
    <div className="a-editor" role="group" aria-label="Data scope">
      <label className="a-radio">
        <input type="radio" name={name} checked={scope.kind === 'all'} onChange={() => onChange(ALL_SCOPE)} />
        <span>
          All job searches
          <span className="a-muted">
            {' '}
            · {runs.length} searches, {total} results (duplicates counted once)
          </span>
        </span>
      </label>
      <label className="a-radio">
        <input
          type="radio"
          name={name}
          checked={scope.kind === 'searches'}
          onChange={() => onChange({ kind: 'searches', runIds: scope.runIds, jobIds: [] })}
          disabled={runs.length === 0}
        />
        <span>Selected searches</span>
      </label>
      {scope.kind === 'searches' && (
        <ul className="a-runs">
          {runs.map((run) => (
            <li key={run.id} className="a-run">
              <label className="a-run__pick">
                <input type="checkbox" checked={scope.runIds.includes(run.id)} onChange={() => toggleRun(run.id)} />
                <span className="a-run__title">{run.title}</span>
                <span className="a-muted">
                  {formatDateTime(run.createdAt)} · {SOURCES[run.source] ?? run.source} · {jobsText(run.resultCount)}
                </span>
              </label>
              {confirming === run.id ? (
                <span className="a-run__confirm">
                  <button type="button" className="link-button link-button--danger" onClick={() => onDeleteRun(run)}>
                    Remove search
                  </button>
                  <button type="button" className="link-button" onClick={() => setConfirming(null)}>
                    Keep
                  </button>
                </span>
              ) : (
                <IconButton label="Remove this search from Analytics" className="icon-button--small" onClick={() => setConfirming(run.id)}>
                  <TrashIcon />
                </IconButton>
              )}
            </li>
          ))}
        </ul>
      )}
      <label className="a-radio">
        <input
          type="radio"
          name={name}
          checked={scope.kind === 'jobs'}
          onChange={() => onChange({ kind: 'jobs', runIds: [], jobIds: scope.jobIds.length ? scope.jobIds : selectedJobs })}
          disabled={scope.kind !== 'jobs' && selectedJobs.length === 0}
        />
        <span>
          Selected jobs
          <span className="a-muted">
            {' '}
            ·{' '}
            {scope.kind === 'jobs'
              ? `${scope.jobIds.length} job${scope.jobIds.length === 1 ? '' : 's'}`
              : selectedJobs.length
                ? `${selectedJobs.length} ticked in the Jobs table`
                : 'tick jobs in the Jobs table first'}
          </span>
        </span>
      </label>
    </div>
  );
}

// ── Filters ───────────────────────────────────────────────────────────

const FIELD_GROUPS: { label: string; fields: FilterField[] }[] = [
  { label: 'Place & work', fields: ['country', 'city', 'work_mode', 'location'] },
  { label: 'Role', fields: ['role', 'title', 'company', 'seniority', 'employment_type', 'source'] },
  { label: 'Money & dates', fields: ['salary', 'date_posted', 'date_discovered'] },
  { label: 'Requirements', fields: ['skill', 'language', 'certification'] },
  { label: 'Profile', fields: ['match', 'skill_gap', 'missing_skill'] },
  { label: 'Data', fields: ['search_run'] },
];


function facetFor(field: FilterField, facets: Facets | null): FacetValue[] {
  if (!facets) return [];
  switch (field) {
    case 'company':
      return facets.companies;
    case 'role':
      return facets.roles;
    case 'country':
      return facets.countries;
    case 'city':
      return facets.cities;
    case 'source':
      return facets.sources;
    case 'skill':
    case 'missing_skill':
      return facets.skills;
    case 'language':
      return facets.languages;
    case 'certification':
      return facets.certifications;
    default:
      return [];
  }
}

function ValuesInput({
  condition,
  facets,
  runs,
  onChange,
}: {
  condition: FilterCondition;
  facets: Facets | null;
  runs: JobSearchRun[];
  onChange: (c: FilterCondition) => void;
}) {
  const listId = useId();
  const [text, setText] = useState('');
  const values = condition.values ?? [];
  const setValues = (next: string[]) => onChange({ ...condition, values: next });
  const add = () => {
    const v = text.trim();
    if (v && !values.includes(v)) setValues([...values, v]);
    setText('');
  };
  const { field, op } = condition;

  if (op === 'known' || op === 'unknown') return null;

  const options = ENUMS[field];
  if (options) {
    const all = { ...options, unknown: 'Unknown' };
    return (
      <div className="a-checks">
        {Object.entries(all).map(([value, label]) => (
          <label key={value} className="a-check">
            <input
              type="checkbox"
              checked={values.includes(value)}
              onChange={() => setValues(values.includes(value) ? values.filter((v) => v !== value) : [...values, value])}
            />
            {label}
          </label>
        ))}
      </div>
    );
  }
  if (field === 'search_run') {
    return (
      <div className="a-checks a-checks--column">
        {runs.map((run) => (
          <label key={run.id} className="a-check">
            <input
              type="checkbox"
              checked={values.includes(String(run.id))}
              onChange={() => {
                const id = String(run.id);
                setValues(values.includes(id) ? values.filter((v) => v !== id) : [...values, id]);
              }}
            />
            {run.title} <span className="a-muted">· {formatDateTime(run.createdAt)}</span>
          </label>
        ))}
      </div>
    );
  }
  if (op === 'within_days' || op === 'older_than_days') {
    return (
      <div className="a-checks">
        {[1, 3, 7, 10, 30].map((days) => (
          <button
            key={days}
            type="button"
            className={`a-chip-button${condition.number === days ? ' a-chip-button--on' : ''}`}
            onClick={() => onChange({ ...condition, number: days })}
          >
            {days === 1 ? '24 hours' : `${days} days`}
          </button>
        ))}
        <input
          className="input a-number"
          type="number"
          min={0}
          aria-label="Days"
          placeholder="days"
          value={condition.number ?? ''}
          onChange={(e) => onChange({ ...condition, number: e.target.value === '' ? null : Number(e.target.value) })}
        />
      </div>
    );
  }
  if (op === 'on_or_after' || op === 'before') {
    return (
      <input
        className="input a-date"
        type="date"
        aria-label="Date"
        value={values[0] ?? ''}
        onChange={(e) => setValues(e.target.value ? [e.target.value] : [])}
      />
    );
  }
  if (op === 'at_least' || op === 'at_most') {
    const currencies = facets?.currencies.map((c) => c.value) ?? [];
    // The amount is always saved with the currency shown beside it.
    const currency = field === 'salary' ? (condition.currency ?? currencies[0] ?? 'EUR') : null;
    return (
      <div className="a-checks">
        <input
          className={`input a-number${field === 'salary' ? ' a-number--wide' : ''}`}
          type="number"
          min={0}
          step={field === 'salary' ? 5000 : 1}
          aria-label={field === 'salary' ? 'Yearly salary' : 'Value'}
          placeholder={field === 'salary' ? 'per year' : field === 'match' ? '%' : 'count'}
          value={condition.number ?? ''}
          onChange={(e) =>
            onChange({ ...condition, currency, number: e.target.value === '' ? null : Number(e.target.value) })
          }
        />
        {field === 'salary' && (
          <select
            className="input input--auto"
            aria-label="Currency"
            value={currency ?? 'EUR'}
            onChange={(e) => onChange({ ...condition, currency: e.target.value })}
          >
            {[...new Set([...currencies, 'EUR', 'USD', 'GBP', 'CHF'])].map((c) => (
              <option key={c} value={c}>
                {c}
              </option>
            ))}
          </select>
        )}
      </div>
    );
  }
  const suggestions = facetFor(field, facets);
  return (
    <div className="a-values">
      {values.map((v) => (
        <span key={v} className="a-chip">
          {v}
          <button type="button" aria-label={`Remove ${v}`} onClick={() => setValues(values.filter((x) => x !== v))}>
            ×
          </button>
        </span>
      ))}
      <input
        className="input a-text"
        list={listId}
        aria-label="Value"
        placeholder={suggestions.length ? 'Type or pick…' : 'Type a value…'}
        value={text}
        onChange={(e) => {
          const v = e.target.value;
          // Picking from the list adds the value directly.
          if (suggestions.some((s) => s.value === v) && !values.includes(v)) {
            setValues([...values, v]);
            setText('');
          } else {
            setText(v);
          }
        }}
        onKeyDown={(e) => {
          if (e.key === 'Enter') {
            e.preventDefault();
            add();
          }
        }}
        onBlur={add}
      />
      <datalist id={listId}>
        {suggestions.map((s) => (
          <option key={s.value} value={s.value}>
            {jobsText(s.count)}
          </option>
        ))}
      </datalist>
    </div>
  );
}

export function FilterEditor({
  filters,
  facets,
  runs,
  onChange,
}: {
  filters: FilterCondition[];
  facets: Facets | null;
  runs: JobSearchRun[];
  onChange: (filters: FilterCondition[]) => void;
}) {
  const [draft, setDraft] = useState<FilterCondition>(() => emptyCondition('country'));
  const [editing, setEditing] = useState<number | null>(null);
  const save = () => {
    if (!conditionReady(draft)) return;
    const clean = { ...draft, values: (draft.values ?? []).filter((v) => v.trim() !== '') };
    onChange(editing == null ? [...filters, clean] : filters.map((f, i) => (i === editing ? clean : f)));
    setEditing(null);
    setDraft(emptyCondition(draft.field));
  };
  return (
    <div className="a-editor" role="group" aria-label="Filters">
      {filters.length === 0 ? (
        <p className="a-muted">No filters: every job in the scope is analyzed.</p>
      ) : (
        <ul className="a-conditions">
          {filters.map((f, i) => (
            <li key={i} className={`a-condition${editing === i ? ' a-condition--editing' : ''}`}>
              {i > 0 && <span className="a-and">and</span>}
              <button
                type="button"
                className="a-condition__text"
                title="Edit"
                onClick={() => {
                  setEditing(i);
                  setDraft(f);
                }}
              >
                {describeCondition(f, runs)}
              </button>
              <IconButton
                label="Remove condition"
                className="icon-button--small"
                onClick={() => {
                  onChange(filters.filter((_, j) => j !== i));
                  if (editing === i) setEditing(null);
                }}
              >
                <CloseIcon />
              </IconButton>
            </li>
          ))}
        </ul>
      )}
      <div className="a-draft">
        <select
          className="input input--auto"
          aria-label="Field"
          value={draft.field}
          onChange={(e) => setDraft(emptyCondition(e.target.value as FilterField))}
        >
          {/* Group names as disabled rows: WebKitGTK's popup mislabels <optgroup>. */}
          {FIELD_GROUPS.map((g) => [
            <option key={g.label} disabled>
              {g.label}
            </option>,
            ...g.fields.map((f) => (
              <option key={f} value={f}>
                {` ${FIELDS[f]}`}
              </option>
            )),
          ])}
        </select>
        <select
          className="input input--auto"
          aria-label="Condition"
          value={draft.op}
          onChange={(e) => setDraft({ ...draft, op: e.target.value as FilterOp, number: null })}
        >
          {FIELD_OPS[draft.field].map((op) => (
            <option key={op} value={op}>
              {OPS[op]}
            </option>
          ))}
        </select>
        <ValuesInput condition={draft} facets={facets} runs={runs} onChange={setDraft} />
        <button type="button" className="button button--secondary" disabled={!conditionReady(draft)} onClick={save}>
          {editing == null ? (
            <>
              <PlusIcon className="button__icon" />
              Add condition
            </>
          ) : (
            'Update'
          )}
        </button>
        {editing != null && (
          <button
            type="button"
            className="button button--ghost"
            onClick={() => {
              setEditing(null);
              setDraft(emptyCondition(draft.field));
            }}
          >
            Cancel
          </button>
        )}
      </div>
      {filters.length > 0 && (
        <button type="button" className="link-button" onClick={() => onChange([])}>
          Clear all filters
        </button>
      )}
    </div>
  );
}

// ── Ranking ───────────────────────────────────────────────────────────

export function RankingEditor({
  ranking,
  limit,
  facets,
  onChange,
  onLimit,
}: {
  ranking: SortCriterion[];
  limit: number | null;
  facets: Facets | null;
  onChange: (ranking: SortCriterion[]) => void;
  onLimit: (limit: number | null) => void;
}) {
  const listId = useId();
  const [key, setKey] = useState<SortKey>('salary');
  const [value, setValue] = useState('');
  const prefer = PREFER_KEYS.includes(key);
  const suggestions =
    key === 'prefer_country'
      ? facets?.countries
      : key === 'prefer_city'
        ? facets?.cities
        : key === 'prefer_company'
          ? facets?.companies
          : key === 'prefer_skill'
            ? facets?.skills
            : key === 'prefer_role'
              ? facets?.roles
              : [];
  const move = (i: number, delta: number) => {
    const next = [...ranking];
    const [item] = next.splice(i, 1);
    if (item) next.splice(i + delta, 0, item);
    onChange(next);
  };
  return (
    <div className="a-editor" role="group" aria-label="Ranking">
      {ranking.length === 0 ? (
        <p className="a-muted">No ranking: jobs appear newest found first.</p>
      ) : (
        <ol className="a-criteria">
          {ranking.map((c, i) => (
            <li key={`${c.key}-${c.value ?? ''}-${i}`} className="a-criterion">
              <span className="a-criterion__rank">{i + 1}</span>
              {/* Reorder buttons first, so they line up from row to row. */}
              <IconButton label="Move up" className="icon-button--small" disabled={i === 0} onClick={() => move(i, -1)}>
                <ChevronUpIcon />
              </IconButton>
              <IconButton
                label="Move down"
                className="icon-button--small"
                disabled={i === ranking.length - 1}
                onClick={() => move(i, 1)}
              >
                <ChevronDownIcon />
              </IconButton>
              <span className="a-criterion__name">
                {RANKING_KEYS[c.key]}
                {c.value ? `: ${c.value}` : ''}
              </span>
              <button
                type="button"
                className="a-chip-button"
                title="Change direction"
                onClick={() =>
                  onChange(ranking.map((x, j) => (j === i ? { ...x, direction: x.direction === 'desc' ? 'asc' : 'desc' } : x)))
                }
              >
                {directionLabel(c.key, c.direction === 'desc')}
              </button>
              <IconButton label="Remove criterion" className="icon-button--small" onClick={() => onChange(ranking.filter((_, j) => j !== i))}>
                <CloseIcon />
              </IconButton>
            </li>
          ))}
        </ol>
      )}
      <div className="a-draft">
        <select className="input input--auto" aria-label="Criterion" value={key} onChange={(e) => setKey(e.target.value as SortKey)}>
          {Object.entries(RANKING_KEYS).map(([k, label]) => (
            <option key={k} value={k}>
              {label}
            </option>
          ))}
        </select>
        {prefer && (
          <>
            <input
              className="input a-text"
              list={listId}
              aria-label="Value to rank first"
              placeholder="e.g. Austria"
              value={value}
              onChange={(e) => setValue(e.target.value)}
            />
            <datalist id={listId}>
              {(suggestions ?? []).map((s) => (
                <option key={s.value} value={s.value} />
              ))}
            </datalist>
          </>
        )}
        <button
          type="button"
          className="button button--secondary"
          disabled={prefer && !value.trim()}
          onClick={() => {
            onChange([...ranking, { key, direction: 'desc', value: prefer ? value.trim() : null }]);
            setValue('');
          }}
        >
          <PlusIcon className="button__icon" />
          Add
        </button>
        <button type="button" className="link-button" onClick={() => onChange(DEFAULT_RANKING)}>
          Reset
        </button>
      </div>
      <label className="a-limit">
        <input type="checkbox" checked={limit != null} onChange={(e) => onLimit(e.target.checked ? 20 : null)} />
        Analyze only the top
        <input
          className="input a-number"
          type="number"
          min={1}
          aria-label="Number of top jobs"
          disabled={limit == null}
          value={limit ?? 20}
          onChange={(e) => onLimit(Math.max(1, Number(e.target.value) || 1))}
        />
        ranked jobs
      </label>
    </div>
  );
}
