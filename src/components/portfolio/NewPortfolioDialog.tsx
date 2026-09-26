import { useState } from 'react';

import { defaultPageSize } from '../../lib/portfolio/layout';
import { TEMPLATES } from '../../lib/portfolio/templates';
import { toApiError } from '../../services/ipc';
import { createPortfolio, type PageSize, type PortfolioDocument, type PortfolioStart } from '../../services/portfolioService';
import { Dialog } from '../ui/Dialog';
import { TemplateThumbnail } from './TemplateThumbnail';

interface NewPortfolioDialogProps {
  initialTemplate: string;
  /** The Custom Profile has content to start from. */
  profileAvailable: boolean;
  onClose: () => void;
  onCreated: (document: PortfolioDocument) => void;
}

/** Name, template, paper and starting content for a new CV. */
export function NewPortfolioDialog({ initialTemplate, profileAvailable, onClose, onCreated }: NewPortfolioDialogProps) {
  const [name, setName] = useState('My CV');
  const [templateId, setTemplateId] = useState(initialTemplate);
  const [pageSize, setPageSize] = useState<PageSize>(defaultPageSize);
  const [start, setStart] = useState<PortfolioStart>(profileAvailable ? 'custom_profile' : 'blank');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const create = async () => {
    setBusy(true);
    setError(null);
    try {
      onCreated(await createPortfolio(name.trim() || 'My CV', templateId, pageSize, start));
    } catch (err) {
      setError(toApiError(err).message);
      setBusy(false);
    }
  };

  return (
    <Dialog
      title="New CV"
      size="wide"
      onClose={onClose}
      actions={
        <>
          {error && <span className="form-error">{error}</span>}
          <button type="button" className="button button--secondary" onClick={onClose}>
            Cancel
          </button>
          <button type="button" className="button button--primary" disabled={busy} onClick={() => void create()}>
            {busy ? 'Creating…' : 'Create CV'}
          </button>
        </>
      }
    >
      <label className="field">
        <span className="field__label">Name</span>
        <input className="input" maxLength={120} value={name} autoFocus onChange={(e) => setName(e.target.value)} />
      </label>
      <fieldset className="field">
        <legend className="field__label">Template</legend>
        <div className="template-picker" role="radiogroup" aria-label="Template">
          {TEMPLATES.map((t) => (
            <button
              key={t.id}
              type="button"
              role="radio"
              aria-checked={t.id === templateId}
              className={t.id === templateId ? 'template-option template-option--selected' : 'template-option'}
              onClick={() => setTemplateId(t.id)}
            >
              <TemplateThumbnail templateId={t.id} name={t.name} />
              <span className="template-option__name">{t.name}</span>
            </button>
          ))}
        </div>
      </fieldset>
      <div className="form__row new-cv__options">
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
                {size === 'a4' ? 'A4' : 'Letter'}
              </button>
            ))}
          </div>
        </fieldset>
        <fieldset className="field field--grow">
          <legend className="field__label">Start with</legend>
          <label className="radio">
            <input type="radio" name="start" checked={start === 'blank'} onChange={() => setStart('blank')} />
            <span>Empty sections</span>
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
              A copy of my Custom Profile and credentials
              <span className="form__hint">
                {profileAvailable
                  ? ' Copied once; the CV and your Profile stay independent.'
                  : ' Your Custom Profile is empty.'}
              </span>
            </span>
          </label>
        </fieldset>
      </div>
    </Dialog>
  );
}
