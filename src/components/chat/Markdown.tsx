import { useEffect, useMemo, useState, type MouseEvent } from 'react';

import { useOptionalBrowser } from '../../app/browser';
import { renderMarkdown } from '../../lib/markdown';
import { openExternalUrl } from '../../services/systemService';

const isWebLink = (href: string) => /^https?:\/\//i.test(href);

interface LinkMenuState {
  href: string;
  x: number;
  y: number;
}

/**
 * Renders model output. Web links open in the ReMa browser next to the
 * content; Ctrl/⌘-click or the right-click menu opens the external browser.
 */
export function Markdown({ source }: { source: string }) {
  const html = useMemo(() => renderMarkdown(source), [source]);
  const browser = useOptionalBrowser();
  const [menu, setMenu] = useState<LinkMenuState | null>(null);

  const external = (href: string) => void openExternalUrl(href).catch(() => {});
  const inReMa = (href: string) => (browser && isWebLink(href) ? browser.openUrl(href) : external(href));

  const linkOf = (event: MouseEvent<HTMLDivElement>) =>
    (event.target as HTMLElement).closest('a')?.getAttribute('href') ?? null;

  const onClick = (event: MouseEvent<HTMLDivElement>) => {
    const href = linkOf(event);
    if (!href) return;
    event.preventDefault();
    if (event.metaKey || event.ctrlKey || event.shiftKey) external(href);
    else inReMa(href);
  };

  const onContextMenu = (event: MouseEvent<HTMLDivElement>) => {
    const href = linkOf(event);
    if (!href || !isWebLink(href)) return;
    event.preventDefault();
    setMenu({ href, x: event.clientX, y: event.clientY });
  };

  return (
    <>
      <div
        className="markdown"
        onClick={onClick}
        onContextMenu={onContextMenu}
        dangerouslySetInnerHTML={{ __html: html }}
      />
      {menu && (
        <LinkMenu
          menu={menu}
          onClose={() => setMenu(null)}
          onOpen={() => inReMa(menu.href)}
          onExternal={() => external(menu.href)}
        />
      )}
    </>
  );
}

function LinkMenu({
  menu,
  onClose,
  onOpen,
  onExternal,
}: {
  menu: LinkMenuState;
  onClose: () => void;
  onOpen: () => void;
  onExternal: () => void;
}) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && onClose();
    document.addEventListener('mousedown', onClose);
    document.addEventListener('keydown', onKey);
    window.addEventListener('scroll', onClose, true);
    window.addEventListener('resize', onClose);
    return () => {
      document.removeEventListener('mousedown', onClose);
      document.removeEventListener('keydown', onKey);
      window.removeEventListener('scroll', onClose, true);
      window.removeEventListener('resize', onClose);
    };
  }, [onClose]);

  const choose = (action: () => void) => () => {
    action();
    onClose();
  };
  return (
    <div
      className="menu__popover"
      role="menu"
      style={{ top: menu.y + 2, left: Math.min(menu.x, window.innerWidth - 230) }}
      // Clicks inside the menu must not count as "outside".
      onMouseDown={(e) => e.stopPropagation()}
    >
      <button type="button" role="menuitem" className="menu__item" onClick={choose(onOpen)}>
        Open in ReMa
      </button>
      <button type="button" role="menuitem" className="menu__item" onClick={choose(onExternal)}>
        Open in external browser
      </button>
      <button
        type="button"
        role="menuitem"
        className="menu__item"
        onClick={choose(() => void navigator.clipboard?.writeText(menu.href).catch(() => {}))}
      >
        Copy link
      </button>
    </div>
  );
}
