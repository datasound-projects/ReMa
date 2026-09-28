import { useCallback, useEffect, useMemo, useRef, useState } from 'react';

import { useHistory } from '../../hooks/useHistory';
import { usePortfolioPdf } from '../../hooks/usePortfolioPdf';
import { PAGE_SIZES, resolveTheme, type LayoutInput } from '../../lib/portfolio/layout';
import { circlePhoto } from '../../lib/portfolio/photo';
import { clearDraft, keepDraft, recoverDraft } from '../../lib/portfolio/recovery';
import { templateById, type Template } from '../../lib/portfolio/templates';
import { toApiError } from '../../services/ipc';
import {
  deletePortfolio,
  duplicatePortfolio,
  exportPortfolioPdf,
  savePortfolio,
  toInput,
  type PortfolioDocument,
  type PortfolioInput,
} from '../../services/portfolioService';
import { ArrowLeftIcon, ChevronLeftIcon, ChevronRightIcon, DownloadIcon, MoreIcon, PanelIcon, RetryIcon, ZoomInIcon, ZoomOutIcon } from '../icons';
import { LazyPdfPages } from '../pdf/LazyPdfPages';
import { Menu } from '../ui/Menu';
import { AiPanel } from './AiPanel';
import { ContentPanel } from './ContentPanel';
import { DesignPanel } from './DesignPanel';
import { DocumentCanvas, type CanvasSelection, type TextSelection } from './canvas/DocumentCanvas';
import { TemplatePreviewDialog } from './TemplatePreviewDialog';

type SaveState = { status: 'saved' | 'pending' | 'saving' } | { status: 'error'; message: string };
type PanelTab = 'content' | 'design' | 'ai';

const SAVE_DELAY = 700;
const PANEL_MIN = 280;
const PANEL_MAX = 620;
const ZOOMS = [0.5, 0.65, 0.8, 1, 1.25, 1.5, 2];

/**
 * Edits one CV or cover letter: a compact toolbar, a collapsible panel
 * (Content, Design, AI Assistant) and the document itself, edited in
 * place or shown as the exact PDF. Changes save automatically; the last
 * unsaved draft is kept in the browser until it is saved.
 */
export function PortfolioEditor({
  document: doc,
  profileAvailable,
  onClose,
  onOpen,
}: {
  document: PortfolioDocument;
  profileAvailable: boolean;
  onClose: () => void;
  onOpen: (id: number) => void;
}) {
  void profileAvailable;
  const [initial] = useState(() => toInput(doc));
  const history = useHistory<PortfolioInput>(initial);
  const draft = history.value;
  const [save, setSave] = useState<SaveState>({ status: 'saved' });
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState<'export' | 'duplicate' | 'delete' | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [recovered, setRecovered] = useState<PortfolioInput | null>(() => recoverDraft(doc.id, doc.updatedAt)?.input ?? null);
  const [tab, setTab] = useState<PanelTab>('content');
  const [panelOpen, setPanelOpen] = useState(true);
  const [panelWidth, setPanelWidth] = useState(() => {
    const stored = Number(localStorage.getItem('rema.portfolio.panelWidth'));
    return stored >= PANEL_MIN && stored <= PANEL_MAX ? stored : 380;
  });
  const [mode, setMode] = useState<'canvas' | 'pdf'>('canvas');
  const [zoom, setZoom] = useState<number | 'fit'>('fit');
  const [pages, setPages] = useState(0);
  const [page, setPage] = useState(1);
  const [selection, setSelection] = useState<CanvasSelection>(null);
  const [textSelection, setTextSelection] = useState<TextSelection | null>(null);
  const [aiRequest, setAiRequest] = useState<TextSelection | null>(null);
  const [previewTemplate, setPreviewTemplate] = useState<Template | null>(null);
  const [photoCircle, setPhotoCircle] = useState<string | undefined>(undefined);
  const pagesRef = useRef<HTMLDivElement>(null);
  const isLetter = (draft.kind ?? 'cv') === 'cover_letter';

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
        if (!pending.current) clearDraft(doc.id);
        setSave(pending.current ? { status: 'pending' } : { status: 'saved' });
      } catch (err) {
        // Keep the draft to retry with the next change.
        pending.current ??= input;
        setSave({ status: 'error', message: toApiError(err).message });
      }
    });
    return chain.current;
  }, [doc.id]);

  const edit = useCallback(
    (update: (d: PortfolioInput) => PortfolioInput, group?: string) => {
      history.set((d) => {
        const next = update(d);
        if (next !== d) {
          pending.current = next;
          keepDraft(doc.id, next);
        }
        return next;
      }, group ?? null);
      setSave({ status: 'pending' });
      setNotice(null);
    },
    [history, doc.id],
  );

  // Undo and redo are edits too: they need saving.
  const undo = () => {
    history.undo();
    setSave({ status: 'pending' });
  };
  const redo = () => {
    history.redo();
    setSave({ status: 'pending' });
  };
  useEffect(() => {
    if (save.status !== 'pending') return;
    pending.current = draft;
    keepDraft(doc.id, draft);
    const timer = window.setTimeout(() => void flush(), SAVE_DELAY);
    return () => window.clearTimeout(timer);
  }, [draft, save.status, flush, doc.id]);

  // Leaving the editor saves what is left.
  useEffect(() => () => void flush(), [flush]);

  // Keyboard: undo/redo everywhere in the editor, escape clears selection.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const meta = event.metaKey || event.ctrlKey;
      if (meta && event.key.toLowerCase() === 'z') {
        event.preventDefault();
        if (event.shiftKey) redo();
        else undo();
      } else if (meta && event.key.toLowerCase() === 'y') {
        event.preventDefault();
        redo();
      } else if (event.key === 'Escape' && !(event.target as HTMLElement).closest('dialog')) {
        setSelection(null);
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [history.undo, history.redo]);

  // The round photo for templates that place one.
  const photo = draft.content.header.photo ?? '';
  useEffect(() => {
    if (!photo) {
      setPhotoCircle(undefined);
      return;
    }
    let active = true;
    circlePhoto(photo)
      .then((url) => active && setPhotoCircle(url))
      .catch(() => active && setPhotoCircle(undefined));
    return () => {
      active = false;
    };
  }, [photo]);

  const layoutInput = useMemo<LayoutInput>(
    () => ({
      name: draft.name,
      kind: draft.kind ?? 'cv',
      templateId: draft.templateId,
      pageSize: draft.pageSize,
      accent: draft.accent,
      style: draft.style ?? null,
      content: draft.content,
      letter: draft.letter ?? null,
      photoCircle,
    }),
    [draft, photoCircle],
  );
  const theme = useMemo(() => resolveTheme(layoutInput), [layoutInput]);
  const template = templateById(draft.templateId, isLetter ? 'letter' : 'cv');
  const pdf = usePortfolioPdf(mode === 'pdf' ? layoutInput : null);

  const exportPdf = async () => {
    setBusy('export');
    setNotice(null);
    try {
      await flush();
      const { renderPdf, toBase64 } = await import('../../lib/portfolio/pdf');
      const bytes = await renderPdf(layoutInput);
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
      clearDraft(doc.id);
      onClose();
    } catch (err) {
      setNotice(toApiError(err).message);
      setBusy(null);
    }
  };

  // Panel resizing by dragging its edge.
  const startResize = (event: React.MouseEvent) => {
    event.preventDefault();
    const startX = event.clientX;
    const startWidth = panelWidth;
    const move = (e: MouseEvent) => setPanelWidth(Math.min(PANEL_MAX, Math.max(PANEL_MIN, startWidth + (e.clientX - startX))));
    const stop = () => {
      window.removeEventListener('mousemove', move);
      window.removeEventListener('mouseup', stop);
      setPanelWidth((w) => {
        localStorage.setItem('rema.portfolio.panelWidth', String(w));
        return w;
      });
    };
    window.addEventListener('mousemove', move);
    window.addEventListener('mouseup', stop);
  };

  const zoomValue = zoom === 'fit' ? 1 : zoom;
  const zoomStep = (delta: 1 | -1) =>
    setZoom((z) => {
      const current = z === 'fit' ? 1 : z;
      const index = ZOOMS.findIndex((v) => Math.abs(v - current) < 0.01);
      return ZOOMS[Math.min(ZOOMS.length - 1, Math.max(0, (index === -1 ? 3 : index) + delta))] ?? 1;
    });
  const goTo = (n: number) => {
    const target = Math.min(Math.max(1, n), Math.max(1, pages));
    setPage(target);
    pagesRef.current?.querySelectorAll('.pdf-pages__page')[target - 1]?.scrollIntoView({ block: 'start', behavior: 'smooth' });
  };

  const onAskAi = useCallback((sel: TextSelection) => {
    setAiRequest({ ...sel });
    setTab('ai');
    setPanelOpen(true);
  }, []);
  const onTextSelection = useCallback((sel: TextSelection | null) => setTextSelection(sel), []);

  return (
    <div className={`studio-editor${panelOpen ? '' : ' studio-editor--collapsed'}`} style={{ '--panel-width': `${panelWidth}px` } as React.CSSProperties}>
      <div className="studio-toolbar" role="toolbar" aria-label={isLetter ? 'Cover letter' : 'CV'}>
        <button type="button" className="button button--ghost button--small" onClick={onClose} title="Back to all documents">
          <ArrowLeftIcon className="button__icon" />
          Documents
        </button>
        <button
          type="button"
          className={panelOpen ? 'icon-button icon-button--small icon-button--active' : 'icon-button icon-button--small'}
          aria-label={panelOpen ? 'Hide panel' : 'Show panel'}
          aria-pressed={panelOpen}
          title={panelOpen ? 'Hide panel' : 'Show panel'}
          onClick={() => setPanelOpen(!panelOpen)}
        >
          <PanelIcon />
        </button>
        <input
          className="input input--small studio-toolbar__name"
          aria-label="Document name"
          maxLength={120}
          value={draft.name}
          onChange={(e) => edit((d) => ({ ...d, name: e.target.value }), 'name')}
        />
        <span className="studio-toolbar__meta" title={template.description}>
          {template.name} · {PAGE_SIZES[draft.pageSize].label}
        </span>
        <div className="studio-toolbar__group" role="group" aria-label="History">
          <button type="button" className="icon-button icon-button--small" aria-label="Undo" title="Undo (⌘Z)" disabled={!history.canUndo} onClick={undo}>
            <span className="studio-toolbar__undo">↶</span>
          </button>
          <button type="button" className="icon-button icon-button--small" aria-label="Redo" title="Redo (⇧⌘Z)" disabled={!history.canRedo} onClick={redo}>
            <span className="studio-toolbar__undo">↷</span>
          </button>
        </div>
        <SaveStatus state={save} onRetry={() => void flush()} />
        <span className="studio-toolbar__spacer" />
        <div className="segmented" role="radiogroup" aria-label="View">
          <button type="button" role="radio" aria-checked={mode === 'canvas'} className={mode === 'canvas' ? 'segmented__option segmented__option--active' : 'segmented__option'} onClick={() => setMode('canvas')}>
            Edit
          </button>
          <button type="button" role="radio" aria-checked={mode === 'pdf'} className={mode === 'pdf' ? 'segmented__option segmented__option--active' : 'segmented__option'} onClick={() => setMode('pdf')}>
            PDF preview
          </button>
        </div>
        <div className="studio-toolbar__group" role="group" aria-label="Zoom">
          <button type="button" className="icon-button icon-button--small" aria-label="Zoom out" onClick={() => zoomStep(-1)}>
            <ZoomOutIcon />
          </button>
          <button type="button" className="link-button studio-toolbar__zoom" onClick={() => setZoom('fit')} title="Fit to width">
            {zoom === 'fit' ? 'Fit' : `${Math.round(zoom * 100)}%`}
          </button>
          <button type="button" className="icon-button icon-button--small" aria-label="Zoom in" onClick={() => zoomStep(1)}>
            <ZoomInIcon />
          </button>
        </div>
        {mode === 'pdf' && pages > 1 && (
          <div className="studio-toolbar__group" role="group" aria-label="Pages">
            <button type="button" className="icon-button icon-button--small" aria-label="Previous page" disabled={page <= 1} onClick={() => goTo(page - 1)}>
              <ChevronLeftIcon />
            </button>
            <span className="studio-toolbar__pages">
              {page} / {pages}
            </span>
            <button type="button" className="icon-button icon-button--small" aria-label="Next page" disabled={page >= pages} onClick={() => goTo(page + 1)}>
              <ChevronRightIcon />
            </button>
          </div>
        )}
        <button type="button" className="button button--primary button--small" disabled={busy !== null} onClick={() => void exportPdf()}>
          <DownloadIcon className="button__icon" />
          {busy === 'export' ? 'Exporting…' : 'Export PDF'}
        </button>
        <Menu
          items={[
            { label: 'Preview template with my content', onSelect: () => setPreviewTemplate(template) },
            { label: 'Duplicate', disabled: busy !== null, onSelect: () => void duplicate() },
            { label: 'Delete', danger: true, onSelect: () => setConfirmDelete(true) },
          ]}
          trigger={(props) => (
            <button type="button" className="icon-button icon-button--small" aria-label="More actions" title="More actions" {...props}>
              <MoreIcon />
            </button>
          )}
        />
      </div>

      {recovered && (
        <div className="notice notice--warning studio-editor__confirm" role="alertdialog" aria-label="Recovered changes">
          <span>Unsaved changes from an earlier session were found for this document.</span>
          <button
            type="button"
            className="button button--primary button--small"
            onClick={() => {
              history.reset(recovered);
              pending.current = recovered;
              setSave({ status: 'pending' });
              setRecovered(null);
            }}
          >
            Restore
          </button>
          <button
            type="button"
            className="button button--ghost button--small"
            onClick={() => {
              clearDraft(doc.id);
              setRecovered(null);
            }}
          >
            Discard
          </button>
        </div>
      )}
      {confirmDelete && (
        <div className="notice notice--danger studio-editor__confirm" role="alertdialog" aria-label="Delete this document">
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
        <p className="notice studio-editor__notice" role="status">
          {notice}
        </p>
      )}

      <div className="studio-editor__panes">
        {panelOpen && (
          <aside className="studio-panel" aria-label="Editor panel">
            <div className="studio-panel__tabs" role="tablist" aria-label="Panel">
              {(
                [
                  ['content', 'Content'],
                  ['design', 'Design'],
                  ['ai', 'AI Assistant'],
                ] as [PanelTab, string][]
              ).map(([id, label]) => (
                <button key={id} type="button" role="tab" aria-selected={tab === id} className={tab === id ? 'studio-panel__tab studio-panel__tab--active' : 'studio-panel__tab'} onClick={() => setTab(id)}>
                  {label}
                </button>
              ))}
            </div>
            <div className="studio-panel__body" role="tabpanel">
              {tab === 'content' && <ContentPanel draft={draft} edit={edit} />}
              {tab === 'design' && <DesignPanel draft={draft} edit={edit} onPreviewTemplate={setPreviewTemplate} />}
              {tab === 'ai' && (
                <AiPanel draft={draft} edit={edit} textSelection={textSelection} canvasSelection={selection} requested={aiRequest} />
              )}
            </div>
            <div className="studio-panel__handle" role="separator" aria-orientation="vertical" aria-label="Resize panel" onMouseDown={startResize} />
          </aside>
        )}

        <div className="studio-stage" aria-label={mode === 'canvas' ? 'Document' : 'PDF preview'}>
          {mode === 'canvas' ? (
            <div className="studio-stage__canvas" style={{ '--stage-zoom': zoomValue } as React.CSSProperties}>
              <DocumentCanvas
                draft={draft}
                theme={theme}
                edit={edit}
                selection={selection}
                onSelect={setSelection}
                onTextSelection={onTextSelection}
                onAskAi={onAskAi}
                zoom={zoomValue}
                photoCircle={photoCircle}
              />
            </div>
          ) : (
            <div className="studio-stage__pdf" ref={pagesRef}>
              <div className="studio-stage__pdf-bar">
                <span>Exactly what will be exported</span>
                {pdf.rendering && <span className="studio-preview__status">Updating…</span>}
                {pdf.error && (
                  <span className="form-error" role="alert">
                    {pdf.error}
                  </span>
                )}
              </div>
              <div className="studio-stage__pages" style={{ width: zoom === 'fit' ? '100%' : `${Math.round(PAGE_SIZES[draft.pageSize].width * zoomValue * 1.3)}px` }}>
                <LazyPdfPages data={pdf.data} label={`Preview of ${draft.name}`} onRendered={setPages} />
              </div>
            </div>
          )}
        </div>
      </div>

      {previewTemplate && (
        <TemplatePreviewDialog
          templateId={previewTemplate.id}
          pageSize={draft.pageSize}
          current={layoutInput}
          actionLabel="Apply"
          onAction={(id) => {
            edit((d) => ({ ...d, templateId: id }));
            setPreviewTemplate(null);
          }}
          onClose={() => setPreviewTemplate(null)}
        />
      )}
    </div>
  );
}

function SaveStatus({ state, onRetry }: { state: SaveState; onRetry: () => void }) {
  if (state.status === 'error') {
    return (
      <span className="studio-save studio-save--error" role="alert" title={state.message}>
        Not saved: {state.message}{' '}
        <button type="button" className="link-button" onClick={onRetry}>
          <RetryIcon /> Retry
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
