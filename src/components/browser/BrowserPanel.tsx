import { useEffect, useLayoutEffect, useRef, useState, type PointerEvent } from 'react';

import { useBrowser } from '../../app/browser';
import { toApiError } from '../../services/ipc';
import {
  browserBack,
  browserForward,
  browserReload,
  openInBrowser,
  setBrowserBounds,
  setBrowserVisible,
  type BrowserBounds,
} from '../../services/browserService';
import { openExternalUrl } from '../../services/systemService';
import {
  ArrowLeftIcon,
  ArrowRightIcon,
  CloseIcon,
  ExternalIcon,
  GlobeIcon,
  MaximizeIcon,
  PanelIcon,
  ReloadIcon,
  RestoreIcon,
} from '../icons';
import { IconButton } from '../ui/IconButton';
import { useAutofill } from '../../hooks/useAutofill';
import { AutofillButton, AutofillReport } from './Autofill';

const MIN_WIDTH = 340;
/** Space always left for ReMa's own content next to a docked browser. */
const MIN_CONTENT = 360;
const COLLAPSED_WIDTH = 44;

/** The element's area in window coordinates, if it has one. */
function boundsOf(el: HTMLElement | null): BrowserBounds | null {
  const rect = el?.getBoundingClientRect();
  if (!rect || rect.width < 2 || rect.height < 2) return null;
  return { x: rect.left, y: rect.top, width: rect.width, height: rect.height };
}

function hostOf(url: string | null): string {
  if (!url) return '';
  try {
    return new URL(url).host;
  } catch {
    return url;
  }
}

/**
 * The browser workspace: ReMa's toolbar plus an area where the isolated
 * browser webview (drawn natively by Rust) is placed.
 */
export function BrowserPanel() {
  const browser = useBrowser();
  const { status, request, dock, mode } = browser;
  const viewportRef = useRef<HTMLDivElement>(null);
  const panelRef = useRef<HTMLElement>(null);
  const [error, setError] = useState<string | null>(null);
  const [address, setAddress] = useState('');
  const [editing, setEditing] = useState(false);
  const autofill = useAutofill(status.url);

  // CSS keeps at least MIN_CONTENT pixels for ReMa's content.
  const width = Math.max(browser.width, MIN_WIDTH);

  // The address field follows the page unless the user is typing.
  const shownAddress = editing ? address : (status.url ?? '');

  // Open the requested page once the area exists.
  const { requestDone } = browser;
  useEffect(() => {
    if (!request || mode === 'collapsed') return;
    const bounds = boundsOf(viewportRef.current);
    if (!bounds) return;
    requestDone(request.seq);
    setError(null);
    openInBrowser(request.url, bounds).catch((err: unknown) => setError(toApiError(err).message));
  }, [request, mode, requestDone]);

  // Keep the native page exactly over the reserved area.
  useLayoutEffect(() => {
    const el = viewportRef.current;
    if (!el || !status.open) return;
    let frame = 0;
    const sync = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        const bounds = boundsOf(el);
        if (bounds) void setBrowserBounds(bounds).catch(() => {});
      });
    };
    const observer = new ResizeObserver(sync);
    observer.observe(el);
    // The page can also move without resizing (e.g. the sidebar collapses):
    // the workspace around it resizes then.
    const workspace = el.closest('.app-shell__workspace');
    if (workspace) observer.observe(workspace);
    window.addEventListener('resize', sync);
    sync();
    return () => {
      cancelAnimationFrame(frame);
      observer.disconnect();
      window.removeEventListener('resize', sync);
    };
  }, [status.open, dock, mode, width, autofill.result, error]);

  const visible = status.open && mode !== 'collapsed' && !browser.covered;
  useEffect(() => {
    if (status.open) void setBrowserVisible(visible).catch(() => {});
  }, [visible, status.open]);

  // Dragging the edge resizes; the page is hidden meanwhile so the native
  // view cannot swallow the pointer.
  const startResize = (event: PointerEvent<HTMLDivElement>) => {
    event.preventDefault();
    const handle = event.currentTarget;
    handle.setPointerCapture(event.pointerId);
    const release = browser.cover();
    const panel = panelRef.current?.getBoundingClientRect();
    const workspace = panelRef.current?.parentElement?.getBoundingClientRect();
    if (!panel || !workspace) return release();
    const maxWidth = Math.max(MIN_WIDTH, workspace.width - MIN_CONTENT);
    const onMove = (e: globalThis.PointerEvent) => {
      const next = dock === 'right' ? panel.right - e.clientX : e.clientX - panel.left;
      browser.setWidth(Math.round(Math.min(Math.max(next, MIN_WIDTH), maxWidth)));
    };
    const onUp = () => {
      handle.removeEventListener('pointermove', onMove);
      handle.removeEventListener('pointerup', onUp);
      handle.removeEventListener('pointercancel', onUp);
      release();
    };
    handle.addEventListener('pointermove', onMove);
    handle.addEventListener('pointerup', onUp);
    handle.addEventListener('pointercancel', onUp);
  };

  const go = () => {
    const target = address.trim();
    setEditing(false);
    if (target) browser.openUrl(target);
  };

  const openExternally = () => {
    if (status.url) void openExternalUrl(status.url).catch(() => {});
  };

  if (mode === 'collapsed') {
    return (
      <aside
        ref={panelRef}
        className={`browser browser--${dock} browser--collapsed`}
        style={{ width: COLLAPSED_WIDTH }}
        aria-label="Browser (collapsed)"
      >
        <IconButton label="Expand browser" className="icon-button--small" onClick={() => browser.setMode('docked')}>
          <GlobeIcon />
        </IconButton>
        <span className="browser__collapsed-title" title={status.title ?? status.url ?? ''}>
          {hostOf(status.url) || 'Browser'}
        </span>
        <IconButton label="Close browser" className="icon-button--small" onClick={browser.close}>
          <CloseIcon />
        </IconButton>
      </aside>
    );
  }

  const maximized = mode === 'maximized';
  return (
    <aside
      ref={panelRef}
      className={`browser browser--${dock}${maximized ? ' browser--maximized' : ''}`}
      style={maximized ? undefined : { width }}
      aria-label="Browser"
    >
      {!maximized && (
        <div
          className="browser__resize"
          role="separator"
          aria-orientation="vertical"
          aria-label="Resize browser"
          onPointerDown={startResize}
        />
      )}
      <div className="browser__header">
        <GlobeIcon className="browser__header-icon" />
        <span className="browser__title" title={status.title ?? ''}>
          {status.title || hostOf(status.url) || 'Browser'}
        </span>
        <IconButton
          label={dock === 'right' ? 'Dock left' : 'Dock right'}
          className="icon-button--small"
          onClick={() => browser.setDock(dock === 'right' ? 'left' : 'right')}
        >
          <PanelIcon side={dock === 'right' ? 'left' : 'right'} />
        </IconButton>
        <IconButton label="Collapse" className="icon-button--small" onClick={() => browser.setMode('collapsed')}>
          <PanelIcon side={dock} />
        </IconButton>
        <IconButton
          label={maximized ? 'Restore' : 'Maximize'}
          className="icon-button--small"
          onClick={() => browser.setMode(maximized ? 'docked' : 'maximized')}
        >
          {maximized ? <RestoreIcon /> : <MaximizeIcon />}
        </IconButton>
        <IconButton label="Close browser" className="icon-button--small" onClick={browser.close}>
          <CloseIcon />
        </IconButton>
      </div>

      <div className="browser__toolbar">
        <IconButton label="Back" className="icon-button--small" disabled={!status.open} onClick={() => void browserBack()}>
          <ArrowLeftIcon />
        </IconButton>
        <IconButton
          label="Forward"
          className="icon-button--small"
          disabled={!status.open}
          onClick={() => void browserForward()}
        >
          <ArrowRightIcon />
        </IconButton>
        <IconButton label="Reload" className="icon-button--small" disabled={!status.open} onClick={() => void browserReload()}>
          <ReloadIcon />
        </IconButton>
        <form
          className="browser__address"
          onSubmit={(e) => {
            e.preventDefault();
            go();
            (document.activeElement as HTMLElement | null)?.blur();
          }}
        >
          <input
            className="browser__address-input"
            aria-label="Address"
            placeholder="Enter a web address"
            spellCheck={false}
            autoFocus={!status.url && !request}
            value={shownAddress}
            onFocus={(e) => {
              setAddress(status.url ?? '');
              setEditing(true);
              e.currentTarget.select();
            }}
            onBlur={() => setEditing(false)}
            onChange={(e) => {
              setAddress(e.target.value);
              setEditing(true);
            }}
            onKeyDown={(e) => {
              if (e.key === 'Escape') {
                setEditing(false);
                e.currentTarget.blur();
              }
            }}
          />
        </form>
        <IconButton
          label="Open in external browser"
          className="icon-button--small"
          disabled={!status.url}
          onClick={openExternally}
        >
          <ExternalIcon />
        </IconButton>
        <AutofillButton autofill={autofill} disabled={!status.open || status.loading} />
      </div>
      {status.loading && <div className="browser__progress" aria-hidden="true" />}

      <AutofillReport autofill={autofill} />

      <div ref={viewportRef} className="browser__viewport">
        {error ? (
          <div className="browser__message" role="alert">
            <p>{error}</p>
            {status.url && (
              <button type="button" className="button button--secondary" onClick={openExternally}>
                <ExternalIcon className="button__icon" />
                Open in external browser
              </button>
            )}
          </div>
        ) : !status.open && !request ? (
          <div className="browser__message">
            <GlobeIcon className="browser__message-icon" />
            <p className="browser__message-title">Browse next to your work</p>
            <p>Enter an address above, or open a link from Chat, a task result or Analytics.</p>
          </div>
        ) : (
          browser.covered && <div className="browser__message">…</div>
        )}
      </div>
      {status.open && (
        <p className="browser__footnote">
          Pages here cannot access ReMa. If a site doesn’t work, use{' '}
          <button type="button" className="link-button" onClick={openExternally}>
            Open in external browser
          </button>
          .
        </p>
      )}
    </aside>
  );
}
