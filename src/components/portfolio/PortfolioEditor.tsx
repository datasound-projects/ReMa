import { useCallback, useEffect, useRef, useState } from 'react';

import { usePortfolioPdf } from '../../hooks/usePortfolioPdf';
import { SECTION_KINDS, addSection, moveSection, removeSection, updateSection } from '../../lib/portfolio/content';
import { ACCENTS, TEMPLATES, templateById } from '../../lib/portfolio/templates';
import { toApiError } from '../../services/ipc';
import {
  deletePortfolio,
  duplicatePortfolio,
  exportPortfolioPdf,
  savePortfolio,
  toInput,
  type PageSize,
  type PortfolioDocument,
  type PortfolioHeader,
  type PortfolioInput,
} from '../../services/portfolioService';
import { ArrowLeftIcon, CheckIcon, ChevronDownIcon, DownloadIcon, MoreIcon, PlusIcon } from '../icons';
import { LazyPdfPages } from '../pdf/LazyPdfPages';
import { Menu } from '../ui/Menu';
import { SectionEditor } from './SectionEditor';
import { TemplateThumbnail } from './TemplateThumbnail';

type SaveState = { status: 'saved' | 'pending' | 'saving' } | { status: 'error'; message: string };

const SAVE_DELAY = 700;

/**
 * Edits one CV. Changes save automatically (shortly after typing stops,
 * and when leaving); the preview is the exported PDF itself.
 */
export function PortfolioEditor({
  document: doc,
  onClose,
  onOpen,
}: {
  document: PortfolioDocument;
  onClose: () => void;
  onOpen: (id: number) => void;
}) {
  const [draft, setDraft] = useState<PortfolioInput>(() => toInput(doc));
  const [save, setSave] = useState<SaveState>({ status: 'saved' });
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState<'export' | 'duplicate' | 'delete' | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);

  // Saving: the latest unsaved draft, written one save at a time.
  const pending = useRef<PortfolioInput | null>(null);
  const chain = useRef<Promise<void>>(Promise.resolve());
  const flush = useCallback((): Promise<void> => {
    chain.current = chain.current.then(async () => {
      const input = pending.current;
      if (!input) return;
      pending.current = null;
      setSave({ status: 'saving' });
      try {
        await savePortfolio(doc.id, input);
        setSave(pending.current ? { status: 'pending' } : { status: 'saved' });
      } catch (err) {
        // Keep the draft to retry with the next change.
        pending.current ??= input;
        setSave({ status: 'error', message: toApiError(err).message });
      }
    });
    return chain.current;
  }, [doc.id]);

  const edit = (update: (d: PortfolioInput) => PortfolioInput) => {
    setDraft((d) => {
      const next = update(d);
      pending.current = next;
      return next;
    });
    setSave({ status: 'pending' });
    setNotice(null);
  };

  useEffect(() => {
    if (save.status !== 'pending') return;
    const timer = window.setTimeout(() => void flush(), SAVE_DELAY);
    return () => window.clearTimeout(timer);
  }, [draft, save.status, flush]);

  // Leaving the editor saves what is left.
  useEffect(() => () => void flush(), [flush]);

  const pdf = usePortfolioPdf(draft);
  const template = templateById(draft.templateId);

  const exportPdf = async () => {
    setBusy('export');
    setNotice(null);
    try {
      await flush();
      const { renderPdf, toBase64 } = await import('../../lib/portfolio/pdf');
      const bytes = await renderPdf(draft);
      const file = await exportPortfolioPdf(doc.id, toBase64(bytes));
      if (file) setNotice(`Exported “${file}”.`);
    } catch (err) {
      setNotice(`Export failed: ${toApiError(err).message}`);
    } finally {
      setBusy(null);
    }
  };

  const duplicate = async () => {
    setBusy('duplicate');
    try {
      await flush();
      const copy = await duplicatePortfolio(doc.id);
      onOpen(copy.id);
    } catch (err) {
      setNotice(toApiError(err).message);
      setBusy(null);
    }
  };

  const remove = async () => {
    setBusy('delete');
    pending.current = null;
    try {
      await deletePortfolio(doc.id);
      onClose();
    } catch (err) {
      setNotice(toApiError(err).message);
      setBusy(null);
    }
  };

  const setHeader = (patch: Partial<PortfolioHeader>) =>
    edit((d) => ({ ...d, content: { ...d.content, header: { ...d.content.header, ...patch } } }));
  const h = draft.content.header;

  return (
    <div className="studio-editor">
      <div className="studio-toolbar" role="toolbar" aria-label="CV">
        <button type="button" className="button button--ghost" onClick={onClose}>
          <ArrowLeftIcon className="button__icon" />
          All CVs
        </button>
        <input
          className="input studio-toolbar__name"
          aria-label="CV name"
          maxLength={120}
          value={draft.name}
          onChange={(e) => edit((d) => ({ ...d, name: e.target.value }))}
        />
        <Menu
          align="start"
          trigger={(props) => (
            <button type="button" className="button button--secondary" {...props}>
              {template.name}
              <ChevronDownIcon className="button__icon" />
            </button>
          )}
        >
          {(close) => (
            <div className="template-menu" role="radiogroup" aria-label="Template">
              {TEMPLATES.map((t) => (
                <button
                  key={t.id}
                  type="button"
                  role="radio"
                  aria-checked={t.id === draft.templateId}
                  className={t.id === draft.templateId ? 'template-option template-option--selected' : 'template-option'}
                  title={t.description}
                  onClick={() => {
                    close();
                    edit((d) => ({ ...d, templateId: t.id }));
                  }}
                >
                  <TemplateThumbnail templateId={t.id} pageSize={draft.pageSize} name={t.name} />
                  <span className="template-option__name">{t.name}</span>
                </button>
              ))}
            </div>
          )}
        </Menu>
        <div className="segmented" role="radiogroup" aria-label="Paper size">
          {(['a4', 'letter'] as PageSize[]).map((size) => (
            <button
              key={size}
              type="button"
              role="radio"
              aria-checked={draft.pageSize === size}
              className={draft.pageSize === size ? 'segmented__option segmented__option--active' : 'segmented__option'}
              onClick={() => edit((d) => ({ ...d, pageSize: size }))}
            >
              {size === 'a4' ? 'A4' : 'Letter'}
            </button>
          ))}
        </div>
        {!template.ats && (
          <Menu
            align="start"
            trigger={(props) => (
              <button type="button" className="button button--ghost" aria-label="Accent color" title="Accent color" {...props}>
                <span className="swatch" style={{ background: draft.accent || template.colors.accent }} />
                <ChevronDownIcon className="button__icon" />
              </button>
            )}
          >
            {(close) => (
              <div className="swatches" role="radiogroup" aria-label="Accent color">
                <button
                  type="button"
                  role="radio"
                  aria-checked={draft.accent === ''}
                  className="swatches__default"
                  onClick={() => {
                    close();
                    edit((d) => ({ ...d, accent: '' }));
                  }}
                >
                  <span className="swatch" style={{ background: template.colors.accent }} />
                  Template color
                </button>
                <div className="swatches__grid">
                  {ACCENTS.map((a) => (
                    <button
                      key={a.value}
                      type="button"
                      role="radio"
                      aria-checked={draft.accent === a.value}
                      aria-label={a.label}
                      title={a.label}
                      className="swatches__option"
                      style={{ background: a.value }}
                      onClick={() => {
                        close();
                        edit((d) => ({ ...d, accent: a.value }));
                      }}
                    >
                      {draft.accent === a.value && <CheckIcon />}
                    </button>
                  ))}
                </div>
              </div>
            )}
          </Menu>
        )}
        <span className="studio-toolbar__spacer" />
        <SaveStatus state={save} onRetry={() => void flush()} />
        <button type="button" className="button button--primary" disabled={busy !== null} onClick={() => void exportPdf()}>
          <DownloadIcon className="button__icon" />
          {busy === 'export' ? 'Exporting…' : 'Export PDF'}
        </button>
        <Menu
          items={[
            { label: 'Duplicate', disabled: busy !== null, onSelect: () => void duplicate() },
            { label: 'Delete', danger: true, onSelect: () => setConfirmDelete(true) },
          ]}
          trigger={(props) => (
            <button type="button" className="icon-button" aria-label="More actions" title="More actions" {...props}>
              <MoreIcon />
            </button>
          )}
        />
      </div>
      {confirmDelete && (
        <div className="notice notice--danger studio-editor__confirm" role="alertdialog" aria-label="Delete this CV">
          <span>Delete “{draft.name}”? Uploaded files are not affected.</span>
          <button type="button" className="button button--danger button--small" disabled={busy === 'delete'} onClick={() => void remove()}>
            Delete
          </button>
          <button type="button" className="button button--ghost button--small" onClick={() => setConfirmDelete(false)}>
            Cancel
          </button>
        </div>
      )}
      {notice && (
        <p className="notice" role="status">
          {notice}
        </p>
      )}

      <div className="studio-editor__panes">
        <div className="studio-editor__form">
          <section className="studio-section" aria-label="Header">
            <div className="studio-section__head">
              <span className="studio-section__static">Header</span>
              <span className="studio-section__kind">Name and contact</span>
            </div>
            <div className="studio-section__body">
              <div className="profile-grid">
                <HeaderField label="Full name" value={h.fullName} onChange={(fullName) => setHeader({ fullName })} />
                <HeaderField label="Headline" value={h.headline} placeholder="Senior Data Engineer" onChange={(headline) => setHeader({ headline })} />
                <HeaderField label="Email" type="email" value={h.email} onChange={(email) => setHeader({ email })} />
                <HeaderField label="Phone" type="tel" value={h.phone} onChange={(phone) => setHeader({ phone })} />
                <HeaderField label="Location" value={h.location} placeholder="City, Country" onChange={(location) => setHeader({ location })} />
                <HeaderField label="Website" type="url" value={h.website} placeholder="https://" onChange={(website) => setHeader({ website })} />
                <HeaderField label="LinkedIn" type="url" value={h.linkedin} placeholder="https://linkedin.com/in/…" onChange={(linkedin) => setHeader({ linkedin })} />
                <HeaderField label="GitHub" type="url" value={h.github} placeholder="https://github.com/…" onChange={(github) => setHeader({ github })} />
              </div>
            </div>
          </section>

          {draft.content.sections.map((section, index) => (
            <SectionEditor
              key={section.id}
              section={section}
              first={index === 0}
              last={index === draft.content.sections.length - 1}
              onChange={(next) => edit((d) => ({ ...d, content: updateSection(d.content, section.id, () => next) }))}
              onMove={(delta) => edit((d) => ({ ...d, content: moveSection(d.content, section.id, delta) }))}
              onRemove={() => edit((d) => ({ ...d, content: removeSection(d.content, section.id) }))}
            />
          ))}

          <Menu
            align="start"
            trigger={(props) => (
              <button
                type="button"
                className="button button--secondary studio-editor__add"
                disabled={draft.content.sections.length >= 30}
                {...props}
              >
                <PlusIcon className="button__icon" />
                Add section
              </button>
            )}
          >
            {(close) => (
              <div className="section-menu">
                {SECTION_KINDS.map((k) => (
                  <button
                    key={k.kind}
                    type="button"
                    role="menuitem"
                    className="menu__item section-menu__item"
                    onClick={() => {
                      close();
                      edit((d) => ({ ...d, content: addSection(d.content, k.kind) }));
                    }}
                  >
                    <span className="section-menu__label">{k.label}</span>
                    <span className="section-menu__description">{k.description}</span>
                  </button>
                ))}
              </div>
            )}
          </Menu>
        </div>

        <aside className="studio-preview" aria-label="Preview">
          <div className="studio-preview__bar">
            <span>
              Preview · {template.name} · {draft.pageSize === 'a4' ? 'A4' : 'Letter'}
            </span>
            {pdf.rendering && <span className="studio-preview__status">Updating…</span>}
          </div>
          {pdf.error && (
            <p className="form-error" role="alert">
              {pdf.error}
            </p>
          )}
          <div className="studio-preview__pages">
            <LazyPdfPages data={pdf.data} label={`Preview of ${draft.name}`} />
          </div>
        </aside>
      </div>
    </div>
  );
}

function HeaderField({
  label,
  value,
  onChange,
  placeholder,
  type = 'text',
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  type?: 'text' | 'email' | 'tel' | 'url';
}) {
  return (
    <label className="field">
      <span className="field__label">{label}</span>
      <input
        className="input"
        type={type}
        value={value}
        placeholder={placeholder}
        maxLength={type === 'url' ? 2000 : 200}
        onChange={(e) => onChange(e.target.value)}
      />
    </label>
  );
}

function SaveStatus({ state, onRetry }: { state: SaveState; onRetry: () => void }) {
  if (state.status === 'error') {
    return (
      <span className="studio-save studio-save--error" role="alert" title={state.message}>
        Not saved: {state.message}{' '}
        <button type="button" className="link-button" onClick={onRetry}>
          Retry
        </button>
      </span>
    );
  }
  return (
    <span className="studio-save" role="status">
      {state.status === 'saved' ? 'Saved' : 'Saving…'}
    </span>
  );
}
