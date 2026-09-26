import { Fragment } from 'react';

import { openExternalUrl } from '../../services/systemService';

const URL = /https?:\/\/[^\s<>"')]+/g;

/**
 * Plain text (an error, a notice) with its web addresses as links that open
 * in the system browser.
 */
export function LinkedText({ text }: { text: string }) {
  const parts: (string | { url: string })[] = [];
  let last = 0;
  for (const match of text.matchAll(URL)) {
    // Sentence punctuation after an address is not part of it.
    const url = match[0].replace(/[.,;:!?]+$/, '');
    const start = match.index;
    parts.push(text.slice(last, start), { url });
    last = start + url.length;
  }
  parts.push(text.slice(last));
  return (
    <>
      {parts.map((part, i) =>
        typeof part === 'string' ? (
          <Fragment key={i}>{part}</Fragment>
        ) : (
          <a
            key={i}
            className="linked-text__link"
            href={part.url}
            onClick={(event) => {
              event.preventDefault();
              void openExternalUrl(part.url).catch(() => {});
            }}
          >
            {part.url.replace(/^https?:\/\//, '')}
          </a>
        ),
      )}
    </>
  );
}
