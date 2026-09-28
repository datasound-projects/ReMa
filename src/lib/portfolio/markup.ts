/**
 * Inline markup on the canvas: the same `**bold**`, `_italic_` and
 * `[text](url)` notation the layout engine prints, turned into HTML for
 * editing and back into text afterwards.
 */
import { inlineRuns } from './layout';

const escape = (s: string) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');

/** One line of text as HTML with <b>, <i> and <a>. */
export function lineToHtml(line: string): string {
  return inlineRuns(line)
    .map((r) => {
      let html = escape(r.text);
      if (r.link) html = `<a href="${escape(r.link)}">${html}</a>`;
      if (r.italics) html = `<i>${html}</i>`;
      if (r.bold) html = `<b>${html}</b>`;
      return html;
    })
    .join('');
}

const BOLD_TAGS = new Set(['B', 'STRONG']);
const ITALIC_TAGS = new Set(['I', 'EM']);

function walk(node: Node, state: { bold: boolean; italic: boolean; link: string | null }): string {
  if (node.nodeType === Node.TEXT_NODE) {
    let text = (node.textContent ?? '').replace(/ /g, ' ').replace(/\n/g, ' ');
    if (!text) return '';
    // Markers only around non-space text, so the notation stays valid.
    const lead = /^\s*/.exec(text)?.[0] ?? '';
    const trail = /\s*$/.exec(text)?.[0] ?? '';
    const core = text.slice(lead.length, text.length - trail.length);
    if (!core) return text;
    text = core;
    if (state.link) text = `[${text}](${state.link})`;
    if (state.italic) text = `_${text}_`;
    if (state.bold) text = `**${text}**`;
    return lead + text + trail;
  }
  if (node.nodeType !== Node.ELEMENT_NODE) return '';
  const el = node as HTMLElement;
  if (el.tagName === 'BR') return '';
  const style = el.style;
  const next = {
    bold: state.bold || BOLD_TAGS.has(el.tagName) || style.fontWeight === 'bold' || Number(style.fontWeight) >= 600,
    italic: state.italic || ITALIC_TAGS.has(el.tagName) || style.fontStyle === 'italic',
    link: state.link ?? (el.tagName === 'A' ? (el.getAttribute('href') ?? null) : null),
  };
  return Array.from(el.childNodes)
    .map((child) => walk(child, next))
    .join('');
}

/** The markup text of an edited line (its HTML element). */
export function htmlToLine(element: HTMLElement): string {
  return walk(element, { bold: false, italic: false, link: null })
    .replace(/\*\*\*\*/g, '')
    .replace(/__/g, '')
    .replace(/\s+$/g, (m) => m.replace(/\n/g, ''));
}

export interface Line {
  bullet: boolean;
  text: string;
}

const BULLET = /^\s*[-•*–]\s+(.*)$/;

export function splitLines(text: string): Line[] {
  if (!text) return [];
  return text.split('\n').map((raw) => {
    const bullet = BULLET.exec(raw);
    return bullet ? { bullet: true, text: bullet[1] ?? '' } : { bullet: false, text: raw };
  });
}

export function joinLines(lines: Line[]): string {
  return lines.map((l) => (l.bullet ? `- ${l.text}` : l.text)).join('\n');
}

/** Wraps the selected part of a plain field value in markers (no DOM). */
export function wrapSelection(value: string, start: number, end: number, marker: '**' | '_'): string {
  if (start === end) return value;
  const inner = value.slice(start, end);
  if (inner.startsWith(marker) && inner.endsWith(marker) && inner.length > marker.length * 2) {
    return value.slice(0, start) + inner.slice(marker.length, -marker.length) + value.slice(end);
  }
  return `${value.slice(0, start)}${marker}${inner}${marker}${value.slice(end)}`;
}
