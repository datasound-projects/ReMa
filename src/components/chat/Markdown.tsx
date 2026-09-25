import { useMemo, type MouseEvent } from 'react';

import { renderMarkdown } from '../../lib/markdown';
import { openExternalUrl } from '../../services/systemService';

/** Renders model output. Links open in the default browser. */
export function Markdown({ source }: { source: string }) {
  const html = useMemo(() => renderMarkdown(source), [source]);

  const onClick = (event: MouseEvent<HTMLDivElement>) => {
    const link = (event.target as HTMLElement).closest('a');
    if (!link) return;
    event.preventDefault();
    const href = link.getAttribute('href');
    if (href) void openExternalUrl(href).catch(() => {});
  };

  return <div className="markdown" onClick={onClick} dangerouslySetInnerHTML={{ __html: html }} />;
}
