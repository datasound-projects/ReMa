import { useState } from 'react';

import { formatDateTime } from '../../lib/format';
import type { BusinessRun, Locations, RunStatus, SourceNote } from '../../services/businessService';
import { AlertIcon, ChevronDownIcon, ChevronRightIcon, ExternalIcon } from '../icons';
import { ChipInput } from '../profile/EntryList';
import { openLink } from './helpers';
import { RUN_STATUS_LABELS, runStatusTone } from './labels';

export function Links({ links }: { links: { label: string; url: string }[] }) {
  if (links.length === 0) return <span className="biz-muted">—</span>;
  return (
    <span className="biz-links">
      {links.map((l) => (
        <button
          key={l.label + l.url}
          type="button"
          className="link-button biz-links__link"
          onClick={(e) => {
            e.stopPropagation();
            openLink(l.url);
          }}
        >
          {l.label}
          <ExternalIcon className="biz-links__icon" aria-hidden="true" />
        </button>
      ))}
    </span>
  );
}

/** Evidence with its source, retrieval time and excerpt; contrary evidence is marked. */
export function SourceNotes({ notes, empty }: { notes: SourceNote[]; empty?: string }) {
  if (notes.length === 0) return empty ? <p className="biz-muted">{empty}</p> : null;
  return (
    <ul className="biz-evidence">
      {notes.map((n, i) => (
        <li key={i} className={n.contrary ? 'is-contrary' : ''}>
          <span className="biz-evidence__label">
            {n.contrary && <span className="badge badge--warning">Contrary</span>}
            {n.url ? (
              <button type="button" className="link-button" onClick={() => openLink(n.url)}>
                {n.label}
              </button>
            ) : (
              n.label
            )}
            <span className="biz-evidence__time"> · retrieved {formatDateTime(n.retrievedAt)}</span>
          </span>
          {n.excerpt && <span className="biz-evidence__excerpt">“{n.excerpt}”</span>}
        </li>
      ))}
    </ul>
  );
}

export function StatusBadge({ status }: { status: RunStatus }) {
  const tone = runStatusTone(status);
  const className = tone === 'neutral' ? 'badge' : `badge badge--${tone}`;
  return <span className={className}>{RUN_STATUS_LABELS[status]}</span>;
}

/**
 * The latest search did not finish (stopped, failed, or interrupted when
 * ReMa closed): say so instead of passing earlier results off as its own.
 */
export function UnfinishedRun({ run, shownAt }: { run: BusinessRun; shownAt: number | null }) {
  const reason =
    run.failures[run.failures.length - 1] ?? (run.status === 'cancelled' ? 'It was stopped.' : 'It did not finish.');
  return (
    <div className="notice notice--warning" role="status">
      <span>
        The last search ({formatDateTime(run.startedAt)}) did not finish. {reason}{' '}
        {shownAt === null ? 'It left no results.' : `Shown below: the results from ${formatDateTime(shownAt)}.`}
      </span>
    </div>
  );
}

/** The backend's own status line while a request runs, with Stop. */
export function RunProgress({ status, fallback, onStop }: { status: string | null; fallback: string; onStop: () => void }) {
  return (
    <div className="biz-progress">
      <span className="biz-progress__text" role="status">
        <span className="spinner" aria-hidden="true" />
        {status ?? fallback}
      </span>
      <button type="button" className="button button--secondary button--small" onClick={onStop}>
        Stop
      </button>
    </div>
  );
}

/** Notes, then what was searched and what could not be (B27). */
export function SearchedDetails({
  notes,
  sources,
  failures,
}: {
  notes: string[];
  sources: string[];
  failures: string[];
}) {
  const [open, setOpen] = useState(false);
  return (
    <>
      {notes.length > 0 && (
        <ul className="biz-notes">
          {notes.map((note) => (
            <li key={note}>{note}</li>
          ))}
        </ul>
      )}
      {(sources.length > 0 || failures.length > 0) && (
        <div className="biz-searched">
          <button
            type="button"
            className="button button--ghost button--small"
            aria-expanded={open}
            onClick={() => setOpen(!open)}
          >
            {open ? <ChevronDownIcon className="button__icon" /> : <ChevronRightIcon className="button__icon" />}
            What ReMa searched
          </button>
          {open && (
            <div className="biz-searched__body">
              {sources.length > 0 && <p>Searched: {sources.join(', ')}</p>}
              {failures.map((f) => (
                <p key={f} className="biz-searched__failed">
                  <AlertIcon className="biz-searched__icon" aria-hidden="true" />
                  {f}
                </p>
              ))}
            </div>
          )}
        </div>
      )}
    </>
  );
}

export function Chips({ items, label }: { items: string[]; label: string }) {
  if (items.length === 0) return null;
  return (
    <ul className="biz-chips" aria-label={label}>
      {items.map((c) => (
        <li key={c} className="biz-chips__chip">
          {c}
        </li>
      ))}
    </ul>
  );
}

/**
 * Countries, regions and cities (OR within the filter, AND with the others,
 * B7). A radius needs a selected place; ReMa never uses device location.
 */
export function LocationsField({ value, onChange }: { value: Locations; onChange: (next: Locations) => void }) {
  const hasPlace = value.cities.length > 0 || value.regions.length > 0;
  return (
    <div className="biz-locations">
      <div className="field">
        <span className="field__label">Countries</span>
        <ChipInput
          label="Countries"
          placeholder="Austria, Germany, DACH…"
          values={value.countries}
          onChange={(countries) => onChange({ ...value, countries })}
        />
      </div>
      <div className="field">
        <span className="field__label">Regions</span>
        <ChipInput
          label="Regions"
          placeholder="Bavaria, Upper Austria…"
          values={value.regions}
          onChange={(regions) => onChange({ ...value, regions })}
        />
      </div>
      <div className="field">
        <span className="field__label">Cities</span>
        <ChipInput
          label="Cities"
          placeholder="Vienna, Munich…"
          values={value.cities}
          onChange={(cities) => onChange({ ...value, cities })}
        />
      </div>
      <label className="field biz-locations__radius">
        <span className="field__label">Radius (km)</span>
        <input
          className="input input--number"
          type="number"
          min={1}
          max={500}
          disabled={!hasPlace}
          title={hasPlace ? undefined : 'Choose a city or region first'}
          value={value.radiusKm ?? ''}
          onChange={(e) => onChange({ ...value, radiusKm: e.target.value ? Number(e.target.value) : null })}
        />
      </label>
    </div>
  );
}

/** "72" over "evidence coverage 80%", or "Insufficient evidence" over its coverage. */
export function FitCell({ score, coverage, shown }: { score: number | null; coverage: number | null; shown: boolean }) {
  const pct = `${Math.round((coverage ?? 0) * 100)}%`;
  return shown && score !== null ? (
    <>
      <strong className="biz-fit">{Math.round(score)}</strong>
      <span className="biz-muted biz-person__title">evidence coverage {pct}</span>
    </>
  ) : (
    <>
      <span>Insufficient evidence</span>
      <span className="biz-muted biz-person__title">coverage {pct}</span>
    </>
  );
}
