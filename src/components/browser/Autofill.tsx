import { useState } from 'react';

import { useBrowser } from '../../app/browser';
import { dataOr } from '../../hooks/useAsyncData';
import type { Autofill } from '../../hooks/useAutofill';
import { useProfile } from '../../hooks/useProfile';
import { attachProfileDocument, type FileField } from '../../services/browserService';
import { toApiError } from '../../services/ipc';
import { revealProfileDocument, type ProfileDocument } from '../../services/profileService';
import { CheckIcon, CloseIcon, SparkleIcon } from '../icons';
import { IconButton } from '../ui/IconButton';

export function AutofillButton({ autofill, disabled }: { autofill: Autofill; disabled: boolean }) {
  return (
    <button
      type="button"
      className="autofill-button"
      disabled={disabled || autofill.running}
      title="Fill this page's form with your Profile. ReMa never submits it."
      onClick={() => void autofill.run()}
    >
      <SparkleIcon className="button__icon" />
      {autofill.running ? (
        'Filling…'
      ) : (
        <span>
          <span className="autofill-button__brand">ReMa </span>Auto Fill
        </span>
      )}
    </button>
  );
}

/** What Auto Fill did, plus the things left for the user. */
export function AutofillReport({ autofill }: { autofill: Autofill }) {
  const browser = useBrowser();
  const documents = dataOr(useProfile().state, null)?.documents ?? [];
  const { result, error } = autofill;
  if (!result && !error) return null;

  return (
    <div className="autofill" role="status">
      <div className="autofill__head">
        <SparkleIcon className="autofill__icon" />
        <span className="autofill__title">
          {error
            ? 'Auto Fill could not run'
            : result && result.filled.length > 0
              ? `Filled ${result.filled.length} ${result.filled.length === 1 ? 'field' : 'fields'}`
              : 'No empty fields matched your Profile'}
        </span>
        <IconButton label="Dismiss" className="icon-button--small" onClick={autofill.dismiss}>
          <CloseIcon />
        </IconButton>
      </div>
      {error && <p className="autofill__line autofill__line--error">{error}</p>}
      {result && (
        <>
          {result.filled.length > 0 && (
            <p className="autofill__line">
              {result.filled.map((f) => f.label).join(', ')}
              {result.kept > 0 && ` · kept ${result.kept} you already filled`}
            </p>
          )}
          {result.missing.length > 0 && (
            <p className="autofill__line autofill__line--muted">
              Not in your Profile: {result.missing.join(', ')}
            </p>
          )}
          {result.files.map((field) => (
            <FileFieldRow key={field.field} field={field} documents={documents} />
          ))}
          {result.questions.length > 0 && (
            <p className="autofill__line">
              <strong>Your answers:</strong> {result.questions.join(' · ')}
            </p>
          )}
          {result.embeddedForms.map((url) => (
            <p key={url} className="autofill__line">
              This form is embedded from {new URL(url).host}.{' '}
              <button type="button" className="link-button" onClick={() => browser.openUrl(url)}>
                Open it directly
              </button>{' '}
              to use Auto Fill.
            </p>
          ))}
          <p className="autofill__line autofill__line--muted">
            Review every answer and submit the application yourself. ReMa never submits.
          </p>
        </>
      )}
    </div>
  );
}

/** Lets the user choose which Profile document to upload. */
function FileFieldRow({ field, documents }: { field: FileField; documents: ProfileDocument[] }) {
  // CVs first for resume fields.
  const sorted = [...documents].sort(
    (a, b) => Number(b.kind === 'cv') - Number(a.kind === 'cv') || b.createdAt - a.createdAt,
  );
  const [choice, setChoice] = useState<number | null>(null);
  const [state, setState] = useState<'idle' | 'busy' | 'done' | 'failed'>('idle');
  const [error, setError] = useState<string | null>(null);
  const selected = choice ?? (field.kind === 'resume' ? sorted.find((d) => d.kind === 'cv')?.id : undefined) ?? null;

  const attach = async () => {
    if (selected === null) return;
    setState('busy');
    setError(null);
    try {
      await attachProfileDocument(field.field, selected);
      setState('done');
    } catch (err) {
      setError(toApiError(err).message);
      setState('failed');
    }
  };

  return (
    <div className="autofill__file">
      <span className="autofill__file-label">{field.label}</span>
      {documents.length === 0 ? (
        <span className="autofill__line--muted">Add documents on the Profile page to upload them here.</span>
      ) : (
        <>
          <select
            className="input input--auto autofill__select"
            aria-label={`Document for ${field.label}`}
            value={selected ?? ''}
            onChange={(e) => {
              setChoice(Number(e.target.value));
              setState('idle');
            }}
          >
            <option value="" disabled>
              Choose a document
            </option>
            {sorted.map((d) => (
              <option key={d.id} value={d.id}>
                {d.name}
              </option>
            ))}
          </select>
          {state === 'done' ? (
            <span className="autofill__attached">
              <CheckIcon className="button__icon" /> Attached
            </span>
          ) : (
            <button
              type="button"
              className="button button--secondary autofill__attach"
              disabled={selected === null || state === 'busy'}
              onClick={() => void attach()}
            >
              {state === 'busy' ? 'Attaching…' : 'Attach'}
            </button>
          )}
        </>
      )}
      {error && (
        <p className="autofill__line autofill__line--error">
          {error}{' '}
          {selected !== null && (
            <button type="button" className="link-button" onClick={() => void revealProfileDocument(selected)}>
              Show file
            </button>
          )}
        </p>
      )}
    </div>
  );
}
