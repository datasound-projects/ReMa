import { useState } from 'react';

import { defaultPageSize, PAGE_SIZES } from '../../lib/portfolio/layout';
import { DEFAULT_LETTER_TEMPLATE_ID, DEFAULT_TEMPLATE_ID, templateById, type Template } from '../../lib/portfolio/templates';
import { toApiError } from '../../services/ipc';
import { createPortfolio, type PageSize, type PortfolioDocument, type PortfolioKind, type PortfolioStart } from '../../services/portfolioService';
import { Dialog } from '../ui/Dialog';
import { TemplateGallery } from './TemplateGallery';
import { TemplatePreviewDialog } from './TemplatePreviewDialog';

interface NewPortfolioDialogProps {
  kind: PortfolioKind;
  initialTemplate?: string;
  initialStart?: PortfolioStart;
  /** The Custom Profile has content to start from. */
  profileAvailable: boolean;
  onClose: () => void;
  onCreated: (document: PortfolioDocument) => void;
}

/** Name, template, paper and starting content for a new CV or cover letter. */
export function NewPortfolioDialog({ kind, initialTemplate, initialStart, profileAvailable, onClose, onCreated }: NewPortfolioDialogProps) {
  const isLetter = kind === 'cover_letter';
  const [name, setName] = useState(isLetter ? 'My cover letter' : 'My CV');
  const [templateId, setTemplateId] = useState(initialTemplate ?? (isLetter ? DEFAULT_LETTER_TEMPLATE_ID : DEFAULT_TEMPLATE_ID));
  const [pageSize, setPageSize] = useState<PageSize>(defaultPageSize);
  const [start, setStart] = useState<PortfolioStart>(initialStart ?? (profileAvailable ? 'custom_profile' : 'blank'));
  const [preview, setPreview] = useState<Template | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const create = async () => {
    setBusy(true);
    setError(null);
    try {
      onCreated(await createPortfolio({ name: name.trim(), kind, templateId, pageSize, start }));
    } catch (err) {
      setError(toApiError(err).message);
      setBusy(false);
    }
  };

  return (
    <Dialog
      title={isLetter ? 'New cover letter' : 'New CV'}
      size="wide"
      onClose={onClose}
      actions={
        <>
          {error && <span className="form-error">{error}</span>}
          <button type="button" className="button button--secondary" onClick={onClose}>
            Cancel
          </button>
          <button type="button" className="button button--primary" disabled={busy} onClick={() => void create()}>
            {busy ? 'Creating…' : isLetter ? 'Create cover letter' : 'Create CV'}
          </button>
        </>
      }
    >
      <div className="new-doc">
        <div className="new-doc__row">
          <label className="field field--grow">
            <span className="field__label">Name</span>
            <input className="input" maxLength={120} value={name} autoFocus onChange={(e) => setName(e.target.value)} />
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
        <fieldset className="field">
          <legend className="field__label">Start with</legend>
          <div className="new-doc__starts">
            <label className="radio">
              <input type="radio" name="start" checked={start === 'blank'} onChange={() => setStart('blank')} />
              <span>Empty {isLetter ? 'letter' : 'sections'}</span>
            </label>
            <label className="radio">
              <input
                type="radio"
                name="start"
                disabled={!profileAvailable}
                checked={start === 'custom_profile'}
                onChange={() => setStart('custom_profile')}
              />
              <span>
                {isLetter ? 'My contact details from the Custom Profile' : 'A copy of my Custom Profile and credentials'}
                <span className="form__hint">
                  {profileAvailable ? ' Copied once; the document and your Profile stay independent.' : ' Your Custom Profile is empty.'}
                </span>
              </span>
            </label>
            <label className="radio">
              <input type="radio" name="start" checked={start === 'sample'} onChange={() => setStart('sample')} />
              <span>
                Sample content
                <span className="form__hint"> A made-up example to see the template filled in; replace it with your own.</span>
              </span>
            </label>
          </div>
        </fieldset>
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
