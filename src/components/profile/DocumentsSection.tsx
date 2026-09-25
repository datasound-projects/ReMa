import { useState } from 'react';

import { useAction } from '../../hooks/useAction';
import { formatDateTime } from '../../lib/format';
import { move, removeAt, replaceAt } from '../../lib/profileMerge';
import {
  addProfileDocument,
  deleteProfileDocument,
  openProfileDocument,
  updateProfileDocument,
  type CustomField,
  type CustomFieldKind,
  type DocumentFormat,
  type DocumentKind,
  type ProfileDocument,
} from '../../services/profileService';
import { ChevronDownIcon, ChevronUpIcon, FileIcon, PlusIcon, TrashIcon, UploadIcon } from '../icons';
import { IconButton } from '../ui/IconButton';
import { SECTION_IDS } from '../../lib/profileSections';
import { Section } from './ProfileSections';

const KIND_LABELS: Record<DocumentKind, string> = {
  cv: 'CV',
  certificate: 'Certificate',
  portfolio: 'Portfolio',
  other: 'Other',
};

const FORMAT_LABELS: Record<DocumentFormat, string> = {
  pdf: 'PDF',
  docx: 'Word',
  text: 'Text',
  markdown: 'Markdown',
  png: 'PNG',
  jpeg: 'JPEG',
};

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

export function DocumentsSection({
  documents,
  onImport,
  importing,
}: {
  documents: ProfileDocument[];
  /** Reads profile details from a document for review. */
  onImport: (document: ProfileDocument) => void;
  importing: number | null;
}) {
  const action = useAction();
  return (
    <Section
      id={SECTION_IDS.documents}
      title="Documents"
      hint="CVs, certificates and portfolios, stored on this computer. Import reads a CV into your Profile."
    >
      {documents.length === 0 && <p className="entries__empty">No documents yet.</p>}
      {documents.map((doc) => (
        <DocumentRow key={doc.id} document={doc} onImport={onImport} importing={importing === doc.id} />
      ))}
      <div className="form__inline">
        <button
          type="button"
          className="button button--ghost entries__add"
          disabled={action.busy}
          onClick={() => void action.run(() => addProfileDocument(null))}
        >
          <UploadIcon className="button__icon" />
          {action.busy ? 'Adding…' : 'Add document'}
        </button>
        <span className="form__hint">PDF, Word (.docx), text, Markdown, PNG or JPEG · up to 20 MB</span>
      </div>
      {action.error && <p className="form-error">{action.error}</p>}
    </Section>
  );
}

function DocumentRow({
  document: doc,
  onImport,
  importing,
}: {
  document: ProfileDocument;
  onImport: (document: ProfileDocument) => void;
  importing: boolean;
}) {
  const action = useAction();
  const [name, setName] = useState<string | null>(null);
  const [confirming, setConfirming] = useState(false);

  const rename = () => {
    const next = name?.trim();
    setName(null);
    if (next && next !== doc.name) void action.run(() => updateProfileDocument(doc.id, next, doc.kind));
  };

  return (
    <div className="document">
      <FileIcon className="document__icon" />
      <div className="document__main">
        {name === null ? (
          <button type="button" className="document__name" title="Rename" onClick={() => setName(doc.name)}>
            {doc.name}
          </button>
        ) : (
          <input
            className="input document__rename"
            aria-label="Document name"
            autoFocus
            value={name}
            onChange={(e) => setName(e.target.value)}
            onBlur={rename}
            onKeyDown={(e) => {
              if (e.key === 'Enter') rename();
              if (e.key === 'Escape') setName(null);
            }}
          />
        )}
        <span className="document__meta">
          {FORMAT_LABELS[doc.format]} · {formatSize(doc.size)} · {doc.originalName} · added{' '}
          {formatDateTime(doc.createdAt)}
        </span>
      </div>
      <select
        className="input input--auto document__kind"
        aria-label="Document type"
        value={doc.kind}
        onChange={(e) =>
          void action.run(() => updateProfileDocument(doc.id, doc.name, e.target.value as DocumentKind))
        }
      >
        {Object.entries(KIND_LABELS).map(([value, label]) => (
          <option key={value} value={value}>
            {label}
          </option>
        ))}
      </select>
      {confirming ? (
        <>
          <button
            type="button"
            className="button button--danger"
            onClick={() => void action.run(() => deleteProfileDocument(doc.id))}
          >
            Remove
          </button>
          <button type="button" className="button button--ghost" onClick={() => setConfirming(false)}>
            Cancel
          </button>
        </>
      ) : (
        <>
          {doc.hasText && (
            <button
              type="button"
              className="button button--ghost"
              disabled={importing}
              title="Read details from this document into your Profile (you review them first)"
              onClick={() => onImport(doc)}
            >
              {importing ? 'Reading…' : 'Import'}
            </button>
          )}
          <button type="button" className="button button--ghost" onClick={() => void openProfileDocument(doc.id)}>
            Open
          </button>
          <IconButton label="Remove document" className="icon-button--small" onClick={() => setConfirming(true)}>
            <TrashIcon />
          </IconButton>
        </>
      )}
      {action.error && <p className="form-error document__error">{action.error}</p>}
    </div>
  );
}

const FIELD_KINDS: Record<CustomFieldKind, string> = { text: 'Text', url: 'URL', file: 'File' };

export function CustomFieldsSection({
  fields,
  documents,
  onChange,
}: {
  fields: CustomField[];
  documents: ProfileDocument[];
  onChange: (fields: CustomField[]) => void;
}) {
  const action = useAction();
  const set = (index: number, patch: Partial<CustomField>) =>
    onChange(replaceAt(fields, index, { ...(fields[index] as CustomField), ...patch }));

  const attachNew = (index: number) =>
    void action.run(async () => {
      const doc = await addProfileDocument(null);
      if (doc) set(index, { documentId: doc.id });
    });

  return (
    <Section
      id={SECTION_IDS.custom}
      title="Custom fields"
      hint="Anything else an application may ask for: portfolio, research profile, certificates…"
    >
      {fields.length === 0 && <p className="entries__empty">No custom fields yet.</p>}
      {fields.map((field, index) => (
        <div key={index} className="form__row profile-row custom-field">
          <label className="field custom-field__label">
            <span className={index > 0 ? 'sr-only' : 'field__label'}>Name</span>
            <input
              className="input"
              placeholder="Portfolio"
              value={field.label}
              onChange={(e) => set(index, { label: e.target.value })}
            />
          </label>
          <label className="field custom-field__kind">
            <span className={index > 0 ? 'sr-only' : 'field__label'}>Type</span>
            <select
              className="input"
              value={field.kind}
              onChange={(e) => set(index, { kind: e.target.value as CustomFieldKind, value: '', documentId: null })}
            >
              {Object.entries(FIELD_KINDS).map(([value, label]) => (
                <option key={value} value={value}>
                  {label}
                </option>
              ))}
            </select>
          </label>
          <label className="field field--grow">
            <span className={index > 0 ? 'sr-only' : 'field__label'}>Value</span>
            {field.kind === 'file' ? (
              <select
                className="input"
                value={field.documentId ?? ''}
                onChange={(e) => {
                  if (e.target.value === 'new') attachNew(index);
                  else set(index, { documentId: e.target.value ? Number(e.target.value) : null });
                }}
              >
                <option value="">Choose a document</option>
                {documents.map((d) => (
                  <option key={d.id} value={d.id}>
                    {d.name} ({KIND_LABELS[d.kind]})
                  </option>
                ))}
                <option value="new">Add a file…</option>
              </select>
            ) : (
              <input
                className="input"
                type={field.kind === 'url' ? 'url' : 'text'}
                placeholder={field.kind === 'url' ? 'https://' : ''}
                value={field.value}
                onChange={(e) => set(index, { value: e.target.value })}
              />
            )}
          </label>
          <div className="profile-row__actions">
            <IconButton
              label="Move up"
              className="icon-button--small"
              disabled={index === 0}
              onClick={() => onChange(move(fields, index, -1))}
            >
              <ChevronUpIcon />
            </IconButton>
            <IconButton
              label="Move down"
              className="icon-button--small"
              disabled={index === fields.length - 1}
              onClick={() => onChange(move(fields, index, 1))}
            >
              <ChevronDownIcon />
            </IconButton>
            <IconButton label="Remove field" className="icon-button--small" onClick={() => onChange(removeAt(fields, index))}>
              <TrashIcon />
            </IconButton>
          </div>
        </div>
      ))}
      <button
        type="button"
        className="button button--ghost entries__add"
        onClick={() => onChange([...fields, { label: '', kind: 'text', value: '', documentId: null }])}
      >
        <PlusIcon className="button__icon" />
        Add field
      </button>
      {action.error && <p className="form-error">{action.error}</p>}
    </Section>
  );
}
