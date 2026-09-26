import { useState, type MouseEvent, type ReactNode } from 'react';

import { useOptionalBrowser } from '../../app/browser';
import type { ToolActivity } from '../../services/chatService';
import { openExternalUrl } from '../../services/systemService';
import { ChevronDownIcon, ChevronRightIcon, GlobeIcon, SearchIcon } from '../icons';

function hostOf(url: string): string {
  try {
    return new URL(url).hostname.replace(/^www\./, '');
  } catch {
    return url;
  }
}

function plural(n: number, one: string, many: string) {
  return `${n} ${n === 1 ? one : many}`;
}

/** Web links open in the ReMa browser; Ctrl/⌘-click opens the external browser. */
function WebLink({ url, children }: { url: string; children: ReactNode }) {
  const browser = useOptionalBrowser();
  const onClick = (event: MouseEvent<HTMLAnchorElement>) => {
    event.preventDefault();
    if (browser && !(event.metaKey || event.ctrlKey || event.shiftKey)) browser.openUrl(url);
    else void openExternalUrl(url).catch(() => {});
  };
  return (
    <a className="web-activity__link" href={url} onClick={onClick} title={url}>
      {children}
    </a>
  );
}

function Entry({ activity: a }: { activity: ToolActivity }) {
  const page = a.kind === 'web_page';
  const running = a.status === 'running';
  const sources = a.sources ?? [];
  const Icon = page ? GlobeIcon : SearchIcon;
  return (
    <li className="web-activity__entry">
      <Icon className="web-activity__icon" aria-hidden="true" />
      <div className="web-activity__main">
        <span className="web-activity__what">
          {page ? (
            <>
              {running ? 'Opening ' : 'Opened '}
              {a.arguments ? <WebLink url={a.arguments}>{hostOf(a.arguments)}</WebLink> : 'a page'}
            </>
          ) : (
            <>
              {running ? 'Searching ' : 'Searched '}
              {a.arguments ? <q>{a.arguments}</q> : 'the web'}
              {!running && sources.length > 0 && (
                <span className="web-activity__count"> · {plural(sources.length, 'result', 'results')}</span>
              )}
            </>
          )}
        </span>
        {a.status === 'failed' && a.detail && <span className="web-activity__error">{a.detail}</span>}
        {!page && sources.length > 0 && (
          <ul className="web-activity__sources">
            {sources.map((s) => (
              <li key={s.url}>
                <WebLink url={s.url}>{s.title || hostOf(s.url)}</WebLink>
                <span className="web-activity__host">{hostOf(s.url)}</span>
              </li>
            ))}
          </ul>
        )}
      </div>
    </li>
  );
}

/**
 * What the model searched and read on the web for this answer (its
 * provider's hosted web search). Collapsed to one line once finished.
 */
export function WebActivity({ activity }: { activity: ToolActivity[] }) {
  const [open, setOpen] = useState(false);
  const unavailable = activity.find((a) => a.status === 'unavailable');
  const entries = activity.filter((a) => a.status !== 'unavailable');
  const latest = entries.filter((a) => a.status === 'running').at(-1);
  const searches = entries.filter((a) => a.kind === 'web_search').length;
  const pages = entries.filter((a) => a.kind === 'web_page').length;

  return (
    <div className="web-activity">
      {unavailable && (
        <p className="web-activity__notice" role="status">
          <GlobeIcon aria-hidden="true" />
          <span>Web search could not be used for this answer{unavailable.detail ? `: ${unavailable.detail}` : '.'}</span>
        </p>
      )}
      {entries.length > 0 && (
        <>
          <button
            type="button"
            className="web-activity__summary"
            aria-expanded={open}
            onClick={() => setOpen(!open)}
          >
            {open ? <ChevronDownIcon aria-hidden="true" /> : <ChevronRightIcon aria-hidden="true" />}
            <SearchIcon aria-hidden="true" />
            <span>
              {latest ? (
                <span className="web-activity__live">
                  {latest.kind === 'web_page' ? 'Reading a page…' : 'Searching the web…'}
                </span>
              ) : (
                'Searched the web'
              )}
              <span className="web-activity__count">
                {' '}
                · {plural(searches, 'search', 'searches')}
                {pages > 0 && ` · ${plural(pages, 'page', 'pages')}`}
              </span>
            </span>
          </button>
          {open && (
            <ul className="web-activity__list">
              {entries.map((a) => (
                <Entry key={a.id} activity={a} />
              ))}
            </ul>
          )}
        </>
      )}
    </div>
  );
}
