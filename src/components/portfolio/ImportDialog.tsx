import { useState } from 'react';

import { dataOr } from '../../hooks/useAsyncData';
import { useProfile } from '../../hooks/useProfile';
import { defaultPageSize, PAGE_SIZES } from '../../lib/portfolio/layout';
import { DEFAULT_LETTER_TEMPLATE_ID, DEFAULT_TEMPLATE_ID, templateById, type Template } from '../../lib/portfolio/templates';
import { toApiError } from '../../services/ipc';
import {
  createPortfolio,
  importPortfolioDocument,
  importPortfolioFile,
  type PageSize,
  type PortfolioDocument,
  type PortfolioImport,
  type PortfolioKind,
} from '../../services/portfolioService';
import { formatSize } from '../../lib/documents';
import { FileIcon, UploadIcon } from '../icons';
import { Dialog } from '../ui/Dialog';
import { TemplateGallery } from './TemplateGallery';
import { TemplatePreviewDialog } from './TemplatePreviewDialog';

interface ImportDialogProps {
  onClose: () => void;
  onCreated: (document: PortfolioDocument) => void;
}

/**
 * Imports an existing CV or cover letter. The file is kept as it is in
 * the Profile; what ReMa reads from it is reviewed, then rebuilt as an
 * editable document in a template.
 */
export function ImportDialog({ onClose, onCreated }: ImportDialogProps) {
  const profile = useProfile();
  const documents = dataOr(profile.state, null)?.documents ?? [];
  const [kind, setKind] = useState<PortfolioKind>('cv');
  const [busy, setBusy] = useState<'file' | 'document' | 'create' | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<PortfolioImport | null>(null);
  const [name, setName] = useState('');
  const [templateId, setTemplateId] = useState(DEFAULT_TEMPLATE_ID);
  const [pageSize, setPageSize] = useState<PageSize>(defaultPageSize);
  const [excluded, setExcluded] = useState<Set<string>>(new Set());
  const [preview, setPreview] = useState<Template | null>(null);

  const read = async (run: () => Promise<PortfolioImport | null>, what: 'file' | 'document') => {
    setBusy(what);
    setError(null);
    try {
      const imported = await run();
      if (imported) {
        setResult(imported);
        setName(imported.documentName);
        setTemplateId(imported.kind === 'cover_letter' ? DEFAULT_LETTER_TEMPLATE_ID : DEFAULT_TEMPLATE_ID);
        setExcluded(new Set());
      }
    } catch (err) {
      setError(toApiError(err).message);
    } finally {
      setBusy(null);
    }
  };

  const create = async () => {
    if (!result) return;
    setBusy('create');
    setError(null);
    try {
      const content = {
        ...result.content,
        sections: result.content.sections
          .filter((s) => !excluded.has(s.id))
          .map((s) => ({ ...s, entries: s.entries.filter((e) => !excluded.has(e.id)) })),
      };
      const doc = await createPortfolio({
        name: name.trim() || result.documentName,
        kind: result.kind,
        templateId,
        pageSize,
        content,
        letter: result.kind === 'cover_letter' ? result.letter : null,
        sourceDocumentId: result.documentId,
      });
      onCreated(doc);
    } catch (err) {
      setError(toApiError(err).message);
      setBusy(null);
    }
  };

  const toggle = (id: string) =>
    setExcluded((s) => {
      const next = new Set(s);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  if (result) {
    const uncertain = new Set(result.uncertain);
    const isLetter = result.kind === 'cover_letter';
    return (
      <Dialog
        title={`Review “${result.documentName}”`}
        size="wide"
        onClose={onClose}
        actions={
          <>
            {error && <span className="form-error">{error}</span>}
            <button type="button" className="button button--ghost" onClick={() => setResult(null)}>
              Back
            </button>
            <button type="button" className="button button--secondary" onClick={onClose}>
              Cancel
            </button>
            <button type="button" className="button button--primary" disabled={busy !== null} onClick={() => void create()}>
              {busy === 'create' ? 'Creating…' : 'Create editable document'}
            </button>
          </>
        }
      >
        <div className="import-review">
          <p className="notice import-review__mode">
            <strong>Rebuilt in a template.</strong> The original file stays unchanged in your Profile; this creates a new, fully
            editable document from its text. Editing the original PDF’s own layout in place is not supported.
            {!result.hasText && ' This file has no text layer, so the document starts empty.'}
          </p>
          {result.notes.length > 0 && (
            <ul className="import-review__notes">
              {result.notes.map((note, i) => (
                <li key={i}>{note}</li>
              ))}
            </ul>
          )}
          {result.model && (
            <p className="form__hint">
              Read by {result.model}. {uncertain.size > 0 ? 'Highlighted entries were unclear in the source: check them after creating the document.' : ''}
            </p>
          )}
          <div className="new-doc__row">
            <label className="field field--grow">
              <span className="field__label">Name</span>
              <input className="input" maxLength={120} value={name} onChange={(e) => setName(e.target.value)} />
            </label>
            <fieldset className="field">
              <legend className="field__label">Paper</legend>
              <div className="segmented" role="radiogroup" aria-label="Paper size">
                {(['a4', 'letter'] as const).map((size) => (
                  <button
                    key={size}
                    type="button"
                    role="radio"
                    aria-checked={pageSize === size}
                    className={pageSize === size ? 'segmented__option segmented__option--active' : 'segmented__option'}
                    onClick={() => setPageSize(size)}
                  >
                    {PAGE_SIZES[size].label}
                  </button>
                ))}
              </div>
            </fieldset>
          </div>
          <div className="import-review__content">
            <div className="import-review__header">
              <strong>{result.content.header.fullName || 'No name read'}</strong>
              <span className="form__hint">
                {[result.content.header.headline, result.content.header.email, result.content.header.phone, result.content.header.location]
                  .filter(Boolean)
                  .join(' · ') || 'No contact details read'}
              </span>
            </div>
            {isLetter ? (
              <dl className="import-review__letter">
                {(
                  [
                    ['Recipient', result.letter.recipientName],
                    ['Company', result.letter.company],
                    ['Position', result.letter.position],
                    ['Subject', result.letter.subject],
                    ['Greeting', result.letter.greeting],
                    ['Body', result.letter.body],
                    ['Closing', result.letter.closing],
                    ['Signature', result.letter.signature],
                  ] as const
                ).map(([label, value]) => (
                  <div key={label} className={uncertain.has(label.toLowerCase()) ? 'import-review__field import-review__field--uncertain' : 'import-review__field'}>
                    <dt>{label}</dt>
                    <dd>{value || <span className="form__hint">not read</span>}</dd>
                  </div>
                ))}
              </dl>
            ) : (
              <ul className="import-review__sections">
                {result.content.sections.map((s) => (
                  <li key={s.id} className="import-review__section">
                    <label className="checkbox">
                      <input type="checkbox" checked={!excluded.has(s.id)} onChange={() => toggle(s.id)} />
                      <strong>{s.title}</strong>
                      <span className="form__hint">
                        {s.entries.length > 0 ? `${s.entries.length} entr${s.entries.length === 1 ? 'y' : 'ies'}` : `${s.text.length} characters`}
                      </span>
                    </label>
                    {s.entries.length > 0 && (
                      <ul className="import-review__entries">
                        {s.entries.map((e) => (
                          <li key={e.id} className={uncertain.has(e.id) ? 'import-review__entry import-review__entry--uncertain' : 'import-review__entry'}>
                            <label className="checkbox">
                              <input type="checkbox" disabled={excluded.has(s.id)} checked={!excluded.has(e.id) && !excluded.has(s.id)} onChange={() => toggle(e.id)} />
                              <span>
                                {[e.title, e.subtitle].filter(Boolean).join(' · ') || e.tags.join(', ') || '(untitled)'}
                                {(e.start || e.end) && <span className="form__hint"> {[e.start, e.end].filter(Boolean).join(' – ')}</span>}
                                {uncertain.has(e.id) && <span className="badge badge--warning import-review__badge">check</span>}
                              </span>
                            </label>
                          </li>
                        ))}
                      </ul>
                    )}
                  </li>
                ))}
                {result.content.sections.length === 0 && <li className="form__hint">No sections were read.</li>}
              </ul>
            )}
          </div>
          <fieldset className="field">
            <legend className="field__label">Template · {templateById(templateId).name}</legend>
            <TemplateGallery
              kind={isLetter ? 'letter' : 'cv'}
              pageSize={pageSize}
              compact
              selectedId={templateId}
              onSelect={(t) => setTemplateId(t.id)}
              onPreview={setPreview}
            />
          </fieldset>
        </div>
        {preview && (
          <TemplatePreviewDialog
            templateId={preview.id}
            pageSize={pageSize}
            current={{
              name: name || result.documentName,
              kind: result.kind,
              templateId,
              pageSize,
              accent: '',
              content: result.content,
              letter: result.letter,
            }}
            actionLabel="Use this template"
            onAction={(id) => {
              setTemplateId(id);
              setPreview(null);
            }}
            onClose={() => setPreview(null)}
          />
        )}
      </Dialog>
    );
  }

  const importable = documents.filter((d) => d.format !== 'png' && d.format !== 'jpeg' && d.format !== 'webp');
  return (
    <Dialog
      title="Import an existing document"
      onClose={onClose}
      actions={
        <>
          {error && <span className="form-error">{error}</span>}
          <button type="button" className="button button--secondary" onClick={onClose}>
            Cancel
          </button>
        </>
      }
    >
      <div className="import-dialog">
        <fieldset className="field">
          <legend className="field__label">What is it?</legend>
          <div className="segmented" role="radiogroup" aria-label="Document type">
            {(['cv', 'cover_letter'] as const).map((k) => (
              <button
                key={k}
                type="button"
                role="radio"
                aria-checked={kind === k}
                className={kind === k ? 'segmented__option segmented__option--active' : 'segmented__option'}
                onClick={() => setKind(k)}
              >
                {k === 'cv' ? 'CV' : 'Cover letter'}
              </button>
            ))}
          </div>
        </fieldset>
        <p className="form__hint">
          PDF, Word (.docx), text and Markdown files. ReMa reads the text into editable sections for you to review; the file itself is kept
          unchanged in your Profile. Scanned PDFs and images have no text layer and cannot be read (ReMa has no OCR).
        </p>
        <button type="button" className="button button--primary" disabled={busy !== null} onClick={() => void read(() => importPortfolioFile(kind), 'file')}>
          <UploadIcon className="button__icon" />
          {busy === 'file' ? 'Reading…' : 'Choose a file…'}
        </button>
        {importable.length > 0 && (
          <div className="import-dialog__documents">
            <span className="field__label">Or a document already in your Profile</span>
            <ul className="import-dialog__list">
              {importable.map((d) => (
                <li key={d.id}>
                  <button
                    type="button"
                    className="import-dialog__document"
                    disabled={busy !== null}
                    onClick={() => void read(() => importPortfolioDocument(d.id, kind), 'document')}
                  >
                    <FileIcon />
                    <span className="import-dialog__document-name">{d.name}</span>
                    <span className="form__hint">
                      {d.format.toUpperCase()} · {formatSize(d.size)}
                      {!d.hasText && ' · no text'}
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          </div>
        )}
      </div>
    </Dialog>
  );
}
