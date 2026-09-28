import { useEffect, useMemo, useRef, useState } from 'react';

import { usePortfolioPdf } from '../../hooks/usePortfolioPdf';
import { PAGE_SIZES, type LayoutInput } from '../../lib/portfolio/layout';
import { sampleInput } from '../../lib/portfolio/thumbnails';
import { CATEGORIES, CV_TEMPLATES, LETTER_TEMPLATES, templateById, type Template } from '../../lib/portfolio/templates';
import type { PageSize } from '../../services/portfolioService';
import { ChevronLeftIcon, ChevronRightIcon, CloseIcon, ZoomInIcon, ZoomOutIcon } from '../icons';
import { LazyPdfPages } from '../pdf/LazyPdfPages';

interface TemplatePreviewDialogProps {
  templateId: string;
  pageSize?: PageSize;
  /** The open document, to preview the template with real content. */
  current?: LayoutInput | null;
  /** Label of the main action, e.g. "Use this template" or "Apply". */
  actionLabel: string;
  onAction: (templateId: string) => void;
  onClose: () => void;
}

const ZOOMS = [0.5, 0.75, 1, 1.25, 1.5, 2];

/**
 * A large preview of one template with sample content or the current
 * document: zoom, fit to page, page navigation, and neighbouring templates.
 */
export function TemplatePreviewDialog({ templateId, pageSize = 'a4', current, actionLabel, onAction, onClose }: TemplatePreviewDialogProps) {
  const dialog = useRef<HTMLDialogElement>(null);
  const pagesRef = useRef<HTMLDivElement>(null);
  const [id, setId] = useState(templateId);
  const [source, setSource] = useState<'sample' | 'current'>(current ? 'current' : 'sample');
  const [zoom, setZoom] = useState<number | 'fit'>('fit');
  const [pages, setPages] = useState(0);
  const [page, setPage] = useState(1);
  const template = templateById(id);
  const list: readonly Template[] = template.kind === 'letter' ? LETTER_TEMPLATES : CV_TEMPLATES;
  const index = list.findIndex((t) => t.id === id);

  useEffect(() => {
    const el = dialog.current;
    if (el && !el.open) el.showModal();
  }, []);

  const input = useMemo<LayoutInput>(() => {
    if (source === 'current' && current) {
      const kind = template.kind === 'letter' ? 'cover_letter' : 'cv';
      return { ...current, kind, templateId: id, pageSize: current.pageSize };
    }
    return sampleInput(id, pageSize);
  }, [source, current, id, pageSize, template.kind]);
  const pdf = usePortfolioPdf(input, 80);

  const step = (delta: number) => {
    const next = list[(index + delta + list.length) % list.length];
    if (next) {
      setId(next.id);
      setPage(1);
    }
  };
  const scale = zoom === 'fit' ? 1 : zoom;
  const goTo = (n: number) => {
    const target = Math.min(Math.max(1, n), Math.max(1, pages));
    setPage(target);
    const canvas = pagesRef.current?.querySelectorAll('.pdf-pages__page')[target - 1];
    canvas?.scrollIntoView({ block: 'start', behavior: 'smooth' });
  };

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'ArrowRight') step(1);
      if (event.key === 'ArrowLeft') step(-1);
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  });

  return (
    <dialog
      ref={dialog}
      className="dialog preview-dialog"
      aria-label={`Preview of ${template.name}`}
      onCancel={(event) => {
        event.preventDefault();
        onClose();
      }}
      onMouseDown={(event) => {
        if (event.target === dialog.current) onClose();
      }}
    >
      <div className="preview-dialog__bar">
        <button type="button" className="icon-button" aria-label="Previous template" onClick={() => step(-1)}>
          <ChevronLeftIcon />
        </button>
        <div className="preview-dialog__title">
          <strong>{template.name}</strong>
          <span className="preview-dialog__meta">
            {template.kind === 'letter' ? 'Cover letter' : template.category} · {template.tags.join(', ')}
          </span>
        </div>
        <button type="button" className="icon-button" aria-label="Next template" onClick={() => step(1)}>
          <ChevronRightIcon />
        </button>
        <span className="studio-toolbar__spacer" />
        {current && (
          <div className="segmented" role="radiogroup" aria-label="Preview content">
            {(['current', 'sample'] as const).map((s) => (
              <button
                key={s}
                type="button"
                role="radio"
                aria-checked={source === s}
                className={source === s ? 'segmented__option segmented__option--active' : 'segmented__option'}
                onClick={() => setSource(s)}
              >
                {s === 'current' ? 'My content' : 'Sample'}
              </button>
            ))}
          </div>
        )}
        <div className="preview-dialog__zoom" role="group" aria-label="Zoom">
          <button type="button" className="icon-button icon-button--small" aria-label="Zoom out" onClick={() => setZoom((z) => ZOOMS[Math.max(0, ZOOMS.indexOf(z === 'fit' ? 1 : z) - 1)] ?? 0.5)}>
            <ZoomOutIcon />
          </button>
          <button type="button" className="link-button preview-dialog__zoom-value" onClick={() => setZoom('fit')} title="Fit to page">
            {zoom === 'fit' ? 'Fit' : `${Math.round(zoom * 100)}%`}
          </button>
          <button type="button" className="icon-button icon-button--small" aria-label="Zoom in" onClick={() => setZoom((z) => ZOOMS[Math.min(ZOOMS.length - 1, ZOOMS.indexOf(z === 'fit' ? 1 : z) + 1)] ?? 2)}>
            <ZoomInIcon />
          </button>
        </div>
        {pages > 1 && (
          <div className="preview-dialog__pages" role="group" aria-label="Pages">
            <button type="button" className="icon-button icon-button--small" aria-label="Previous page" disabled={page <= 1} onClick={() => goTo(page - 1)}>
              <ChevronLeftIcon />
            </button>
            <span>
              {page} / {pages}
            </span>
            <button type="button" className="icon-button icon-button--small" aria-label="Next page" disabled={page >= pages} onClick={() => goTo(page + 1)}>
              <ChevronRightIcon />
            </button>
          </div>
        )}
        <button type="button" className="button button--primary" onClick={() => onAction(id)}>
          {actionLabel}
        </button>
        <button type="button" className="icon-button" aria-label="Close" onClick={onClose}>
          <CloseIcon />
        </button>
      </div>
      <div className={zoom === 'fit' ? 'preview-dialog__body preview-dialog__body--fit' : 'preview-dialog__body'} ref={pagesRef}>
        {pdf.error && (
          <p className="form-error" role="alert">
            {pdf.error}
          </p>
        )}
        <div className="preview-dialog__sheet" style={zoom === 'fit' ? undefined : { width: `${Math.round(PAGE_SIZES[input.pageSize].width * scale)}px` }}>
          <LazyPdfPages data={pdf.data} label={`Preview of ${template.name}`} onRendered={(n) => setPages(n)} />
        </div>
        {pdf.rendering && <span className="preview-dialog__status">Rendering…</span>}
      </div>
      <p className="preview-dialog__hint">
        {template.description} {CATEGORIES.includes(template.category as never) ? 'Any template works for any profession; switching keeps your content.' : ''}
      </p>
    </dialog>
  );
}
