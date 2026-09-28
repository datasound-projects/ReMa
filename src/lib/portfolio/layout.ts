/**
 * The layout engine: turns a Portfolio Studio document and a template into
 * a pdfmake document definition. Pure (no pdfmake runtime), so the preview,
 * the thumbnails, the export and the tests all use exactly the same layout.
 */
import type { Column, Content, ContentText, TDocumentDefinitions } from 'pdfmake/interfaces';

import type {
  CoverLetter,
  PageSize,
  PortfolioContent,
  PortfolioEntry,
  PortfolioHeader,
  PortfolioKind,
  PortfolioSection,
  PortfolioStyle,
  SectionKind,
} from '../../services/portfolioService';
import { FONT_FAMILIES } from './fonts';
import { applyStyle, luminance } from './style';
import { templateById, type Template } from './templates';

/** Page sizes in points (1/72 inch). */
export const PAGE_SIZES: Record<PageSize, { width: number; height: number; label: string }> = {
  a4: { width: 595.28, height: 841.89, label: 'A4' },
  letter: { width: 612, height: 792, label: 'US Letter' },
};

/** Letter where it is the standard paper size, A4 elsewhere. */
export function defaultPageSize(language = typeof navigator === 'undefined' ? 'en' : navigator.language): PageSize {
  const region = (language.split('-')[1] ?? '').toUpperCase();
  return ['US', 'CA', 'MX', 'PH', 'CL', 'CO', 'VE'].includes(region) ? 'letter' : 'a4';
}

export interface LayoutInput {
  name: string;
  kind?: PortfolioKind;
  templateId: string;
  pageSize: PageSize;
  /** `#rrggbb`, or empty for the template's own accent. */
  accent: string;
  style?: PortfolioStyle | null;
  content: PortfolioContent;
  letter?: CoverLetter | null;
  /** The header photo already masked to a circle (prepared by the caller; the layout is synchronous). */
  photoCircle?: string;
}

/** Everything the builders need about the current design. */
export interface Theme {
  t: Template;
  accent: string;
  heading: string;
  tag: string;
  panel: string;
  panelText: string;
  panelMuted: string;
  subtitleItalic: boolean;
  /** Height of the page's writable area (for keep-together decisions). */
  pageBody: number;
  pageWidth: number;
}

// ── Text helpers ─────────────────────────────────────────────────────

const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];

/** `2021-03` → "Mar 2021"; anything else is shown as typed. */
export function formatDate(value: string): string {
  const v = value.trim();
  const match = /^(\d{4})-(\d{1,2})(?:-\d{1,2})?$/.exec(v);
  if (!match) return v;
  const month = MONTHS[Number(match[2]) - 1];
  return month ? `${month} ${match[1]}` : v;
}

export function period(e: Pick<PortfolioEntry, 'start' | 'end'>): string {
  const start = formatDate(e.start);
  const end = formatDate(e.end);
  if (start && end) return start === end ? start : `${start} – ${end}`;
  return start || end;
}

/** `https://www.example.com/me/` → `example.com/me`. */
export function displayUrl(url: string): string {
  return url
    .trim()
    .replace(/^https?:\/\//i, '')
    .replace(/^www\./i, '')
    .replace(/\/$/, '');
}

const hasText = (s: string) => s.trim().length > 0;

export function entryHasContent(e: PortfolioEntry): boolean {
  return (
    [e.title, e.subtitle, e.location, e.start, e.end, e.url, e.description].some(hasText) ||
    e.tags.some(hasText)
  );
}

/** Sections that appear in the document: visible and with some content. */
export function printedSections(content: PortfolioContent): PortfolioSection[] {
  return content.sections.filter((s) => s.visible && (hasText(s.text) || s.entries.some(entryHasContent)));
}

/** Mixes a `#rrggbb` color with white (`amount` 0–1 of white). */
export function tint(hex: string, amount: number): string {
  return mix(hex, '#ffffff', amount);
}

/** Mixes two `#rrggbb` colors (`amount` 0–1 of the second). */
export function mix(a: string, b: string, amount: number): string {
  const pa = /^#?([0-9a-f]{6})$/i.exec(a.trim());
  const pb = /^#?([0-9a-f]{6})$/i.exec(b.trim());
  if (!pa || !pb) return '#f2f2f2';
  const na = parseInt(pa[1] ?? '000000', 16);
  const nb = parseInt(pb[1] ?? 'ffffff', 16);
  const channel = (shift: number) => {
    const ca = (na >> shift) & 0xff;
    const cb = (nb >> shift) & 0xff;
    return Math.round(ca + (cb - ca) * amount)
      .toString(16)
      .padStart(2, '0');
  };
  return `#${channel(16)}${channel(8)}${channel(0)}`;
}

/** Non-breaking spaces keep a short tag on one line. */
const chipText = (tag: string) => {
  const t = tag.trim();
  const inner = t.length <= 22 ? t.replace(/ /g, ' ') : t;
  return ` ${inner} `;
};

// ── Inline markup ────────────────────────────────────────────────────

const INLINE = /(\*\*[^*\n]+\*\*|__[^_\n]+__|(?<![\w*])\*[^*\n]+\*(?![\w*])|(?<![\w_])_[^_\n]+_(?![\w_])|\[[^\]\n]+\]\((?:https?:\/\/|mailto:)[^)\s]+\))/g;

interface Run {
  text: string;
  bold?: boolean;
  italics?: boolean;
  link?: string;
  color?: string;
  decoration?: 'underline';
}

/**
 * `**bold**`, `_italic_` and `[text](https://…)` inside a line of text.
 * Returns plain runs; nothing else is interpreted.
 */
export function inlineRuns(text: string, linkColor?: string): Run[] {
  const runs: Run[] = [];
  let last = 0;
  for (const match of text.matchAll(INLINE)) {
    const start = match.index ?? 0;
    if (start > last) runs.push({ text: text.slice(last, start) });
    const token = match[0];
    if (token.startsWith('**') || token.startsWith('__')) runs.push({ text: token.slice(2, -2), bold: true });
    else if (token.startsWith('*') || token.startsWith('_')) runs.push({ text: token.slice(1, -1), italics: true });
    else {
      const link = /^\[([^\]]+)\]\(([^)]+)\)$/.exec(token);
      if (link) runs.push({ text: link[1] ?? '', link: link[2], ...(linkColor ? { color: linkColor } : {}) });
      else runs.push({ text: token });
    }
    last = start + token.length;
  }
  if (last < text.length) runs.push({ text: text.slice(last) });
  return runs.length ? runs : [{ text }];
}

/** The text without markup (for width estimates and plain output). */
export function stripMarkup(text: string): string {
  return inlineRuns(text)
    .map((r) => r.text)
    .join('');
}

// ── Height estimate (keep-together decisions) ───────────────────────

/**
 * A deliberately high estimate of an entry's height. Entries estimated
 * well under half a page are kept together on one page; longer ones may
 * break across pages (pdfmake cannot split a kept-together block, so it
 * is never used for content that could exceed a page).
 */
function estimateHeight(e: PortfolioEntry, width: number, th: Theme): number {
  const { size, lineHeight } = th.t;
  const charsPerLine = Math.max(10, width / (size.body * 0.62));
  const lines = (text: string) =>
    text
      .split('\n')
      .reduce((n, line) => n + Math.max(1, Math.ceil(line.length / charsPerLine)), 0);
  const body =
    lines(e.description) + (e.tags.length > 0 ? lines(e.tags.join(' · ')) : 0) + lines(`${e.subtitle} ${e.location}`);
  return (body + 2) * size.body * lineHeight * 1.2 + size.title * 3;
}

const keepTogether = (th: Theme, height: number) => height < th.pageBody * 0.4;

// ── Rich text (descriptions) ─────────────────────────────────────────

const BULLET = /^\s*[-•*–]\s+(.*)$/;

/** Paragraphs, with lines starting with "- " as bullet lists. */
function richText(text: string, th: Theme, color = th.t.colors.text): Content[] {
  const out: Content[] = [];
  const linkColor = th.t.plain ? undefined : th.accent;
  const align = th.t.justify ? { alignment: 'justify' as const } : {};
  let paragraph: string[] = [];
  let bullets: string[] = [];
  const flushParagraph = () => {
    if (paragraph.length) {
      out.push({
        text: paragraph.flatMap((line, i) => [...(i > 0 ? [{ text: '\n' }] : []), ...inlineRuns(line, linkColor)]),
        color,
        margin: [0, 2, 0, 0],
        ...align,
      });
    }
    paragraph = [];
  };
  const flushBullets = () => {
    if (bullets.length) {
      out.push({
        ul: bullets.map((b) => ({ text: inlineRuns(b, linkColor), color })),
        markerColor: th.t.plain ? th.t.colors.text : th.accent,
        margin: [2, 2, 0, 0],
        ...align,
      });
    }
    bullets = [];
  };
  for (const raw of text.split('\n')) {
    const line = raw.trimEnd();
    const bullet = BULLET.exec(line);
    if (bullet) {
      flushParagraph();
      bullets.push(bullet[1] ?? '');
    } else if (line.trim() === '') {
      flushParagraph();
      flushBullets();
    } else {
      flushBullets();
      paragraph.push(line.trim());
    }
  }
  flushParagraph();
  flushBullets();
  return out;
}

// ── Section titles ───────────────────────────────────────────────────

type Margin = [number, number, number, number];

const rule = (width: number, color: string, weight = 0.6, margin: Margin = [0, 2, 0, 7], dash?: { length: number; space: number }): Content => ({
  canvas: [{ type: 'line', x1: 0, y1: 0, x2: width, y2: 0, lineWidth: weight, lineColor: color, ...(dash ? { dash } : {}) }],
  margin,
});

function sectionTitle(title: string, th: Theme, width: number, side = false, index = 0): Content {
  const { t } = th;
  const heading = t.fonts.heading;
  const color = side ? th.panelText : th.heading;
  const accent = side && th.panelText !== t.colors.text ? th.panelText : th.accent;
  const sideStyles: Template['section'][] = ['caps', 'rule', 'plain', 'thin-caps', 'mono', 'serif'];
  const style = side && !sideStyles.includes(t.section) ? 'caps' : t.section;
  const upper = title.toUpperCase();
  const ruleColor = side ? mix(th.panel, th.panelText, 0.35) : t.colors.rule;
  switch (style) {
    case 'rule':
      return {
        stack: [
          { text: upper, font: heading, fontSize: t.size.section, color, characterSpacing: 1.1 },
          rule(width, ruleColor),
        ],
      };
    case 'caps':
      return {
        text: upper,
        font: heading,
        fontSize: t.size.section,
        color: accent,
        characterSpacing: 1.2,
        margin: [0, 0, 0, 6],
      };
    case 'thin-caps':
      return {
        stack: [
          { text: upper, font: t.fonts.body, fontSize: t.size.section, color: accent, characterSpacing: 2 },
          rule(width, ruleColor, 0.5, [0, 3, 0, 8]),
        ],
      };
    case 'bar':
      return {
        columns: [
          {
            width: 9,
            canvas: [{ type: 'rect', x: 0, y: 1.5, w: 3, h: t.size.section * 0.95, r: 1.5, color: accent }],
          },
          { width: '*', text: title, font: heading, fontSize: t.size.section, color },
        ],
        margin: [0, 0, 0, 7],
      };
    case 'side-rule':
      return {
        stack: [
          {
            columns: [
              { width: 8, canvas: [{ type: 'rect', x: 0, y: 0, w: 2, h: t.size.section * 1.15, color: accent }] },
              { width: '*', text: title, font: heading, fontSize: t.size.section, color },
            ],
          },
          rule(width, ruleColor, 0.5, [0, 3, 0, 7]),
        ],
      };
    case 'underline':
      return {
        stack: [
          { text: title, font: heading, fontSize: t.size.section, color },
          rule(26, accent, 2, [0, 3, 0, 8]),
        ],
      };
    case 'mono':
      return {
        stack: [
          {
            text: [
              { text: '// ', color: side ? th.panelMuted : tint(th.accent, 0.35) },
              { text: title.toLowerCase(), color: accent },
            ],
            font: t.fonts.label ?? heading,
            fontSize: t.size.section,
            bold: true,
          },
          rule(width, ruleColor, 0.5, [0, 3, 0, 7]),
        ],
      };
    case 'serif':
      return {
        text: title,
        font: heading,
        fontSize: t.size.section,
        italics: true,
        color: side ? color : th.heading !== t.colors.text ? th.heading : th.accent,
        margin: [0, 0, 0, 6],
      };
    case 'label':
      return {
        text: upper,
        font: heading,
        fontSize: t.size.section,
        color: accent,
        characterSpacing: 0.9,
        lineHeight: 1.2,
      };
    case 'numbered':
      return {
        stack: [
          {
            text: [
              { text: `${String(index + 1).padStart(2, '0')}  `, color: accent, font: t.fonts.label ?? heading },
              { text: title, color },
            ],
            font: heading,
            fontSize: t.size.section,
          },
          rule(width, ruleColor, 0.5, [0, 3, 0, 7]),
        ],
      };
    case 'boxed':
      return {
        table: {
          widths: ['*'],
          body: [[{ text: upper, font: heading, fontSize: t.size.section, color: '#ffffff', characterSpacing: 1, fillColor: accent }]],
        },
        layout: {
          hLineWidth: () => 0,
          vLineWidth: () => 0,
          paddingLeft: () => 7,
          paddingRight: () => 7,
          paddingTop: () => 3.5,
          paddingBottom: () => 3,
        },
        margin: [0, 0, 0, 8],
      };
    case 'smallcaps':
      return {
        columns: [
          { width: '*', canvas: [{ type: 'line', x1: 0, y1: t.size.section * 0.62, x2: Math.max(10, width / 2 - 60), y2: t.size.section * 0.62, lineWidth: 0.5, lineColor: ruleColor }] },
          { width: 'auto', text: upper, font: heading, fontSize: t.size.section, color, characterSpacing: 2, alignment: 'center', margin: [8, 0, 8, 0] },
          { width: '*', canvas: [{ type: 'line', x1: 0, y1: t.size.section * 0.62, x2: Math.max(10, width / 2 - 60), y2: t.size.section * 0.62, lineWidth: 0.5, lineColor: ruleColor }] },
        ],
        margin: [0, 0, 0, 8],
      };
    case 'plain':
      return {
        text: t.plain ? upper : title,
        font: heading,
        bold: true,
        fontSize: t.size.section,
        color,
        margin: [0, 0, 0, 5],
      };
  }
}

// ── Entries ──────────────────────────────────────────────────────────

function titleText(e: PortfolioEntry, th: Theme, fallback = ''): ContentText {
  const text = e.title.trim() || e.subtitle.trim() || fallback;
  return {
    text,
    font: th.t.fonts.heading,
    fontSize: th.t.size.title,
    color: th.t.colors.text,
    ...(e.url.trim() && !th.t.plain ? { link: e.url.trim() } : {}),
  };
}

function subtitleLine(parts: string[], th: Theme, color = th.t.colors.muted): Content | null {
  const text = parts.map((p) => p.trim()).filter(Boolean).join(' · ');
  if (!text) return null;
  return { text, color, italics: th.subtitleItalic, fontSize: th.t.size.body, margin: [0, 1, 0, 0] };
}

function tagsLine(tags: string[], th: Theme, label: string | null, side = false): Content | null {
  const clean = tags.map((t) => t.trim()).filter(Boolean);
  if (clean.length === 0) return null;
  if (th.t.skills === 'tags' && !th.t.plain && !side) {
    return {
      text: clean.flatMap((tag, i) => [
        ...(i > 0 ? [{ text: '  ' }] : []),
        { text: chipText(tag), background: th.tag, color: th.t.colors.text },
      ]),
      fontSize: th.t.size.small,
      lineHeight: 1.55,
      margin: [0, 3, 0, 0],
    };
  }
  return {
    text: [...(label ? [{ text: `${label}: `, bold: true }] : []), { text: clean.join(' · ') }],
    fontSize: th.t.size.small,
    color: side ? th.panelMuted : th.t.colors.muted,
    margin: [0, 2, 0, 0],
  };
}

/** Experience, education, projects, certifications and custom entries. */
function timelineEntry(e: PortfolioEntry, kind: SectionKind, th: Theme, width: number, side = false): Content {
  const { t } = th;
  const dates = period(e);
  const hasTitle = hasText(e.title);
  const subParts = [hasTitle ? e.subtitle : '', e.location];
  const textColor = side ? th.panelText : t.colors.text;
  const mutedColor = side ? th.panelMuted : t.colors.muted;
  const body: Content[] = [
    ...richText(e.description, th, textColor),
    ...(kind === 'projects' ? [tagsLine(e.tags, th, 'Tools', side)] : [tagsLine(e.tags, th, null, side)]).filter(
      (c): c is Content => c !== null,
    ),
  ];
  const url =
    e.url.trim() && (t.plain || kind === 'certifications')
      ? [{ text: t.plain ? e.url.trim() : displayUrl(e.url), link: e.url.trim(), color: mutedColor, fontSize: t.size.small }]
      : [];
  const title = { ...titleText(e, th), color: textColor };
  const sub = subtitleLine(subParts, th, mutedColor);
  const entryStyle = side ? 'stacked' : t.entry;

  let stack: Content[];
  if ((entryStyle === 'dates-left' || entryStyle === 'table' || entryStyle === 'timeline') && !t.plain) {
    const dateWidth = entryStyle === 'table' ? 62 : 74;
    const dateCol: Column = { width: dateWidth, text: dates, fontSize: t.size.small, color: mutedColor, margin: [0, 1, 0, 0] };
    const dot: Column[] =
      entryStyle === 'timeline'
        ? [{ width: 10, canvas: [{ type: 'ellipse', x: 3, y: t.size.title * 0.55, r1: 2.4, r2: 2.4, color: th.accent }] }]
        : [];
    stack = [
      {
        columns: [
          dateCol,
          ...dot,
          {
            width: '*',
            stack: [title, sub, ...body, ...url].filter((c): c is Content => c !== null),
          },
        ],
        columnGap: entryStyle === 'timeline' ? 4 : 12,
      },
    ];
  } else if (entryStyle === 'dates-right' && !t.plain && dates) {
    stack = [
      {
        columns: [
          { width: '*', ...title },
          { width: 'auto', text: dates, fontSize: t.size.small, color: mutedColor, margin: [10, 1.5, 0, 0] },
        ],
      },
      sub,
      ...body,
      ...url,
    ].filter((c): c is Content => c !== null);
  } else {
    stack = [title, subtitleLine([...subParts, dates], th, mutedColor), ...body, ...url].filter(
      (c): c is Content => c !== null,
    );
  }
  const content: Content = { stack, margin: [0, 0, 0, t.gap.entry] };
  return keepTogether(th, estimateHeight(e, width, th)) ? { stack: [content], unbreakable: true } : content;
}

/** Skills: groups of tags, optionally with a group name. */
function skillGroups(entries: PortfolioEntry[], th: Theme, side: boolean): Content[] {
  const { t } = th;
  const groups = entries.filter((e) => e.tags.some(hasText) || hasText(e.title));
  return groups.map((g, i): Content => {
    const label = g.title.trim();
    const tags = g.tags.map((x) => x.trim()).filter(Boolean);
    const margin: Margin = [0, 0, 0, i < groups.length - 1 ? 6 : 0];
    if (side && t.skills === 'list') {
      return {
        stack: [
          ...(label ? [{ text: label, bold: true, fontSize: t.size.small, color: th.panelText, margin: [0, 0, 0, 2] } as Content] : []),
          ...tags.map((tag): Content => ({ text: tag, fontSize: t.size.body, color: th.panelText, margin: [0, 0, 0, 1.5] })),
        ],
        margin,
      };
    }
    if (t.skills === 'tags' && !t.plain && !side) {
      return {
        stack: [
          ...(label ? [{ text: label, bold: true, fontSize: t.size.small, color: t.colors.muted } as Content] : []),
          tagsLine(tags, th, null) ?? { text: '' },
        ],
        margin,
      };
    }
    return {
      text: [...(label ? [{ text: `${label}: `, bold: true }] : []), { text: tags.join(t.plain ? ', ' : ' · ') }],
      color: side ? th.panelText : t.colors.text,
      margin,
    };
  });
}

function languageLines(entries: PortfolioEntry[], th: Theme, side: boolean): Content[] {
  const { t } = th;
  const items = entries.filter((e) => hasText(e.title));
  const muted = side ? th.panelMuted : t.colors.muted;
  const color = side ? th.panelText : t.colors.text;
  if (!side) {
    return [
      {
        text: items.flatMap((e, i) => [
          ...(i > 0 ? [{ text: t.plain ? ', ' : '  ·  ', color: muted }] : []),
          { text: e.title.trim(), bold: true },
          ...(hasText(e.subtitle) ? [{ text: ` (${e.subtitle.trim()})`, color: muted }] : []),
        ]),
        color,
      },
    ];
  }
  return items.map(
    (e): Content => ({
      text: [
        { text: e.title.trim(), bold: true },
        ...(hasText(e.subtitle) ? [{ text: `  ${e.subtitle.trim()}`, color: muted }] : []),
      ],
      color,
      margin: [0, 0, 0, 2],
    }),
  );
}

function linkLines(entries: PortfolioEntry[], th: Theme, side: boolean): Content[] {
  const { t } = th;
  const linkColor = side ? th.panelText : t.plain ? t.colors.text : th.accent;
  return entries
    .filter((e) => hasText(e.url) || hasText(e.title))
    .map((e): Content => {
      const label = e.title.trim();
      const url = e.url.trim();
      const shown = t.plain ? url : displayUrl(url);
      if (side) {
        return {
          stack: [
            ...(label ? [{ text: label, bold: true, fontSize: t.size.small, color: th.panelText } as Content] : []),
            ...(url ? [{ text: shown, link: url, color: linkColor, fontSize: t.size.small, wordBreak: 'break-all', decoration: 'underline' } as Content] : []),
          ],
          margin: [0, 0, 0, 4],
        };
      }
      return {
        text: [
          ...(label ? [{ text: url ? `${label}: ` : label, bold: true }] : []),
          ...(url ? [{ text: shown, link: url, color: linkColor }] : []),
        ],
        margin: [0, 0, 0, 2],
      };
    });
}

function certificationLines(entries: PortfolioEntry[], th: Theme, side: boolean): Content[] {
  const { t } = th;
  const color = side ? th.panelText : t.colors.text;
  const muted = side ? th.panelMuted : t.colors.muted;
  return entries.filter(entryHasContent).map(
    (e): Content => ({
      stack: [
        { text: e.title.trim() || e.subtitle.trim(), bold: true, fontSize: t.size.body, color, ...(e.url.trim() ? { link: e.url.trim() } : {}) },
        ...[subtitleLine([hasText(e.title) ? e.subtitle : '', period(e)], th, muted)]
          .filter((c): c is Content => c !== null)
          .map((c) => ({ ...(c as object), fontSize: t.size.small }) as Content),
      ],
      margin: [0, 0, 0, 5],
    }),
  );
}

/** Publications: title, venue, year and a link, as a reference line. */
function publicationLines(entries: PortfolioEntry[], th: Theme, side: boolean): Content[] {
  const { t } = th;
  const color = side ? th.panelText : t.colors.text;
  const muted = side ? th.panelMuted : t.colors.muted;
  return entries.filter(entryHasContent).map((e): Content => {
    const year = period(e);
    const url = e.url.trim();
    return {
      stack: [
        {
          text: [
            { text: e.title.trim() || 'Untitled', bold: true, ...(url && !t.plain ? { link: url } : {}) },
            ...(hasText(e.subtitle) ? [{ text: `. ${e.subtitle.trim()}`, italics: true, color: muted }] : []),
            ...(year ? [{ text: `, ${year}`, color: muted }] : []),
            ...(hasText(e.location) ? [{ text: `. ${e.location.trim()}`, color: muted }] : []),
          ],
          color,
        },
        ...richText(e.description, th, color),
        ...(url ? [{ text: t.plain ? url : displayUrl(url), link: url, color: t.plain ? color : th.accent, fontSize: t.size.small } as Content] : []),
      ],
      margin: [0, 0, 0, Math.max(4, t.gap.entry - 3)],
    };
  });
}

/** The body of one section (without its title). */
function sectionBody(s: PortfolioSection, th: Theme, width: number, side: boolean): Content[] {
  const entries = s.entries.filter(entryHasContent);
  const text = hasText(s.text) ? richText(s.text, th, side ? th.panelText : th.t.colors.text) : [];
  switch (s.kind) {
    case 'summary':
      return [...text, ...entries.map((e) => timelineEntry(e, s.kind, th, width, side))];
    case 'skills':
      return [...text, ...skillGroups(entries, th, side)];
    case 'languages':
      return [...text, ...languageLines(entries, th, side)];
    case 'links':
      return [...text, ...linkLines(entries, th, side)];
    case 'publications':
      return [...text, ...publicationLines(entries, th, side)];
    case 'certifications':
      return side
        ? [...text, ...certificationLines(entries, th, side)]
        : [...text, ...entries.map((e) => timelineEntry(e, s.kind, th, width, side))];
    default:
      return [...text, ...entries.map((e) => timelineEntry(e, s.kind, th, width, side))];
  }
}

function divider(th: Theme, width: number, side: boolean): Content | null {
  const style = th.t.dividers ?? 'none';
  if (style === 'none' || th.t.plain) return null;
  const color = side ? mix(th.panel, th.panelText, 0.3) : th.t.colors.rule;
  const gap = th.t.gap.section / 2;
  switch (style) {
    case 'hairline':
      return rule(width, color, 0.5, [0, gap, 0, gap]);
    case 'dotted':
      return rule(width, color, 0.8, [0, gap, 0, gap], { length: 1, space: 2.5 });
    case 'thick':
      return rule(width, side ? color : th.accent, 1.6, [0, gap, 0, gap]);
  }
}

function section(s: PortfolioSection, th: Theme, width: number, side: boolean, first: boolean, index: number, last: boolean): Content {
  const { t } = th;
  const dividerAfter = last ? null : divider(th, width, side);
  const margin: Margin = [0, first ? 0 : dividerAfter ? 0 : t.gap.section, 0, 0];
  if (t.section === 'label' && !side) {
    const labelWidth = 92;
    const block: Content = {
      columns: [
        { width: labelWidth, ...(sectionTitle(s.title, th, labelWidth, side, index) as object) } as Content,
        { width: '*', stack: sectionBody(s, th, width - labelWidth - 14, side) },
      ],
      columnGap: 14,
      margin,
    };
    return dividerAfter ? { stack: [block, dividerAfter] } : block;
  }
  const title = { ...(sectionTitle(s.title, th, width, side, index) as object), headlineLevel: 1 } as Content;
  const body = sectionBody(s, th, width, side);
  const [firstPart, ...rest] = body;
  // The title always travels with the start of its first entry.
  const head: Content =
    firstPart !== undefined && isSmall(firstPart)
      ? { stack: [title, firstPart], unbreakable: true }
      : { stack: [title, ...(firstPart !== undefined ? [firstPart] : [])] };
  return { stack: [head, ...rest, ...(dividerAfter ? [dividerAfter] : [])], margin };
}

/** Body parts known to be short (kept together already, or a short paragraph). */
function isSmall(part: Content): boolean {
  if (typeof part === 'object' && part !== null && 'unbreakable' in part) return true;
  if (typeof part === 'object' && part !== null && 'text' in part) {
    const text = part.text;
    if (typeof text === 'string') return text.length < 700;
    if (Array.isArray(text)) return (text as unknown[]).reduce<number>((n, r) => n + (typeof r === 'string' ? r.length : String((r as Run).text ?? '').length), 0) < 700;
  }
  return false;
}

// ── Header ───────────────────────────────────────────────────────────

interface ContactItem {
  text: string;
  link?: string;
}

function contactItems(h: PortfolioHeader, plain: boolean): ContactItem[] {
  const url = (value: string): ContactItem | null =>
    hasText(value) ? { text: plain ? value.trim() : displayUrl(value), link: value.trim() } : null;
  return [
    hasText(h.email) ? { text: h.email.trim(), link: `mailto:${h.email.trim()}` } : null,
    hasText(h.phone) ? { text: h.phone.trim() } : null,
    hasText(h.location) ? { text: h.location.trim() } : null,
    url(h.website),
    url(h.linkedin),
    url(h.github),
  ].filter((c): c is ContactItem => c !== null);
}

function nameText(h: PortfolioHeader, th: Theme, color: string, align?: 'center'): Content | null {
  if (!hasText(h.fullName)) return null;
  const { t } = th;
  return {
    text: h.fullName.trim(),
    font: t.fonts.name,
    bold: t.fonts.name === 'Inter' || t.fonts.name === 'SourceSerif' || t.fonts.name === 'SourceSans' || t.fonts.name === 'Playfair' || t.fonts.name === 'CodeMono',
    fontSize: t.size.name,
    color,
    characterSpacing: t.fonts.name === 'CodeMono' ? 0 : -0.2,
    lineHeight: 1.1,
    ...(align ? { alignment: align } : {}),
  };
}

function headlineText(h: PortfolioHeader, th: Theme, color: string, align?: 'center'): Content | null {
  if (!hasText(h.headline)) return null;
  return {
    text: h.headline.trim(),
    fontSize: th.t.size.headline,
    color,
    margin: [0, 3, 0, 0],
    ...(align ? { alignment: align } : {}),
  };
}

function contactLine(items: ContactItem[], th: Theme, color: string, linkColor: string, align?: 'center'): Content | null {
  if (items.length === 0) return null;
  const sep = th.t.plain ? '  |  ' : '   ·   ';
  return {
    text: items.flatMap((c, i) => [
      ...(i > 0 ? [{ text: sep, color }] : []),
      c.link ? { text: c.text, link: c.link, color: linkColor } : { text: c.text, color },
    ]),
    fontSize: th.t.size.small,
    margin: [0, 7, 0, 0],
    ...(align ? { alignment: align } : {}),
  };
}

/** The photo, when the document has one and the template places it. */
function photo(input: LayoutInput, th: Theme, size: number): Content | null {
  const { t } = th;
  const showPhoto = input.style?.showPhoto ?? false;
  const src = input.content.header.photo?.trim() ?? '';
  if (!showPhoto || !src || t.plain || !src.startsWith('data:image/')) return null;
  const circle = t.photo === 'circle' && input.photoCircle?.startsWith('data:image/');
  return { image: circle ? (input.photoCircle as string) : src, width: size, height: size };
}

/** Rough height of the band header (name, headline, contact), for the drawn band. */
function bandHeight(h: PortfolioHeader, th: Theme, width: number, withContact: boolean, paddingTop: number, paddingBottom: number): number {
  const { t } = th;
  const charsPerLine = (size: number, factor: number) => Math.max(8, width / (size * factor));
  const nameLines = hasText(h.fullName) ? Math.ceil(h.fullName.trim().length / charsPerLine(t.size.name, 0.58)) : 0;
  const headlineLines = hasText(h.headline) ? Math.ceil(h.headline.trim().length / charsPerLine(t.size.headline, 0.55)) : 0;
  const contact = withContact ? contactItems(h, false) : [];
  const contactChars = contact.reduce((n, c) => n + c.text.length + 7, 0);
  const contactLines = contact.length ? Math.ceil(contactChars / charsPerLine(t.size.small, 0.55)) : 0;
  return (
    paddingTop +
    nameLines * t.size.name * 1.15 +
    (headlineLines ? 3 + headlineLines * t.size.headline * t.lineHeight : 0) +
    (contactLines ? 7 + contactLines * t.size.small * t.lineHeight : 0) +
    paddingBottom
  );
}

interface HeaderResult {
  content: Content[];
  /** Height of a drawn band on page 1 (0 for none). */
  band: number;
}

function header(input: LayoutInput, th: Theme, width: number, withContact = true): HeaderResult {
  const { t } = th;
  const h = input.content.header;
  const items = withContact ? contactItems(h, !!t.plain) : [];
  const nameColor = t.accentName ? th.accent : th.heading;
  const headlineColor = t.plain || t.id === 'systems-minimal' ? t.colors.muted : th.accent;
  const gap = t.gap.section;
  const clean = (list: (Content | null)[]) => list.filter((c): c is Content => c !== null);
  const photoSize = Math.round(t.size.name * 2.6);
  const image = photo(input, th, photoSize);
  const withPhoto = (block: Content, align: 'left' | 'right' = 'right'): Content =>
    image
      ? {
          columns: align === 'right' ? [{ width: '*', ...(block as object) } as Column, { width: photoSize, ...(image as object), margin: [14, 0, 0, 0] } as Column] : [{ width: photoSize, ...(image as object), margin: [0, 0, 14, 0] } as Column, { width: '*', ...(block as object) } as Column],
          columnGap: 0,
        }
      : block;

  switch (t.header) {
    case 'center':
      return {
        band: 0,
        content: [
          ...(image ? [{ ...(image as object), alignment: 'center', margin: [0, 0, 0, 8] } as Content] : []),
          {
            stack: clean([
              nameText(h, th, nameColor, 'center'),
              headlineText(h, th, headlineColor, 'center'),
              contactLine(items, th, t.colors.muted, t.colors.muted, 'center'),
            ]),
          },
          rule(width, t.colors.rule, 0.8, [0, 10, 0, gap - 4]),
        ],
      };
    case 'split':
      return {
        band: 0,
        content: [
          withPhoto(
            {
              columns: [
                { width: '*', stack: clean([nameText(h, th, nameColor), headlineText(h, th, headlineColor)]) },
                {
                  width: 'auto',
                  stack: items.map(
                    (c): Content => ({
                      text: c.text,
                      ...(c.link ? { link: c.link } : {}),
                      alignment: 'right',
                      fontSize: t.size.small,
                      color: t.colors.muted,
                      margin: [0, 0, 0, 1],
                    }),
                  ),
                  margin: [12, 2, 0, 0],
                },
              ],
            },
            'left',
          ),
          rule(width, th.accent, 1.2, [0, 10, 0, gap - 4]),
        ],
      };
    case 'stacked': {
      const thick = t.dividers === 'thick';
      return {
        band: 0,
        content: [
          withPhoto({
            stack: clean([
              nameText(h, th, nameColor),
              headlineText(h, th, headlineColor),
              contactLine(items, th, t.colors.muted, t.colors.muted),
            ]),
          }),
          rule(width, thick ? th.accent : t.colors.rule, thick ? 2.2 : 0.8, [0, 12, 0, gap - 2]),
        ],
      };
    }
    case 'monogram': {
      const initials = h.fullName
        .trim()
        .split(/\s+/)
        .filter(Boolean)
        .slice(0, 2)
        .map((w) => w[0]?.toUpperCase() ?? '')
        .join('');
      const box = Math.round(t.size.name * 1.7);
      const mark: Column = image
        ? ({ width: photoSize, ...(image as object) } as Column)
        : {
            width: box,
            table: {
              widths: [box],
              heights: [box],
              body: [
                [
                  {
                    text: initials || '·',
                    font: t.fonts.name,
                    bold: true,
                    fontSize: Math.round(t.size.name * 0.7),
                    color: '#ffffff',
                    fillColor: th.accent,
                    alignment: 'center',
                    margin: [0, Math.round(box * 0.22), 0, 0],
                  },
                ],
              ],
            },
            layout: 'noBorders',
          };
      return {
        band: 0,
        content: [
          {
            columns: [
              mark,
              {
                width: '*',
                stack: clean([
                  nameText(h, th, nameColor),
                  headlineText(h, th, headlineColor),
                  contactLine(items, th, t.colors.muted, t.colors.muted),
                ]),
                margin: [0, 2, 0, 0],
              },
            ],
            columnGap: 14,
            margin: [0, 0, 0, gap],
          },
        ],
      };
    }
    case 'band': {
      const [left, top, right] = t.margins;
      const soft = tint(th.accent, 0.78);
      const paddingTop = Math.max(26, top - 6);
      const paddingBottom = 22;
      const textWidth = width - (image ? photoSize + 14 : 0);
      const height = bandHeight(h, th, textWidth, withContact, paddingTop, paddingBottom);
      const block: Content = {
        stack: clean([nameText(h, th, '#ffffff'), headlineText(h, th, soft), contactLine(items, th, soft, '#ffffff')]),
      };
      return {
        band: height,
        content: [
          {
            table: {
              widths: ['*'],
              heights: [Math.max(0, height - paddingTop - paddingBottom)],
              body: [[withPhoto(block)]],
            },
            layout: {
              hLineWidth: () => 0,
              vLineWidth: () => 0,
              paddingLeft: () => left,
              paddingRight: () => right,
              paddingTop: () => paddingTop,
              paddingBottom: () => paddingBottom,
            },
            // Bleeds to the page edges; the band itself is drawn in the page background.
            margin: [-left, -top, -right, gap],
          },
        ],
      };
    }
    case 'left':
    default:
      return {
        band: 0,
        content: [
          {
            stack: [
              withPhoto({
                stack: clean([
                  nameText(h, th, nameColor),
                  headlineText(h, th, headlineColor),
                  contactLine(items, th, t.colors.muted, t.plain ? t.colors.text : t.colors.muted),
                ]),
              }),
            ],
            margin: [0, 0, 0, gap],
          },
        ],
      };
  }
}

/** Contact details as a side-panel block (sidebar layout). */
function sideContact(input: LayoutInput, th: Theme, width: number): Content[] {
  const items = contactItems(input.content.header, false);
  const image = photo(input, th, Math.min(width - 10, 96));
  const blocks: Content[] = [];
  if (image) blocks.push({ ...(image as object), alignment: 'center', margin: [0, 0, 0, 12] } as Content);
  if (items.length === 0) return blocks;
  return [
    ...blocks,
    sectionTitle('Contact', th, width, true),
    {
      stack: items.map(
        (c): Content => ({
          text: c.text,
          color: th.panelText,
          ...(c.link ? { link: c.link } : {}),
          fontSize: th.t.size.small,
          wordBreak: 'break-all',
          margin: [0, 0, 0, 3],
        }),
      ),
    },
  ];
}

// ── Patterns ─────────────────────────────────────────────────────────

type Canvas = NonNullable<Extract<Content, { canvas: unknown }>['canvas']>;

/** A subtle pattern inside a rectangle (decorative areas only). */
function pattern(kind: Template['pattern'], x: number, y: number, w: number, h: number, color: string): Canvas {
  const out: Canvas = [];
  if (!kind || kind === 'none') return out;
  if (kind === 'dots') {
    const step = 14;
    for (let py = y + 7; py < y + h; py += step) {
      for (let px = x + 7; px < x + w; px += step) out.push({ type: 'ellipse', x: px, y: py, r1: 0.9, r2: 0.9, color });
    }
  } else if (kind === 'grid') {
    const step = 18;
    for (let px = x; px <= x + w; px += step) out.push({ type: 'line', x1: px, y1: y, x2: px, y2: y + h, lineWidth: 0.35, lineColor: color });
    for (let py = y; py <= y + h; py += step) out.push({ type: 'line', x1: x, y1: py, x2: x + w, y2: py, lineWidth: 0.35, lineColor: color });
  } else if (kind === 'diagonal') {
    const step = 16;
    for (let d = -h; d < w; d += step) {
      const x1 = Math.max(x, x + d);
      const y1 = d < 0 ? y - d : y;
      const x2 = Math.min(x + w, x + d + h);
      const y2 = y + (x2 - (x + d));
      if (x2 > x1) out.push({ type: 'line', x1, y1, x2, y2, lineWidth: 0.35, lineColor: color });
    }
  }
  return out;
}

// ── Document ─────────────────────────────────────────────────────────

export function resolveTheme(input: Pick<LayoutInput, 'templateId' | 'accent' | 'pageSize' | 'style' | 'kind'>): Theme {
  const base = templateById(input.templateId, input.kind === 'cover_letter' ? 'letter' : 'cv');
  const t = applyStyle(base, input.style, input.accent);
  const accent = t.colors.accent;
  const page = PAGE_SIZES[input.pageSize];
  const panel = t.colors.panel && (accent === base.colors.accent || t.colors.panel !== base.colors.panel) ? t.colors.panel : tint(accent, 0.93);
  const darkPanel = luminance(panel) < 0.3;
  return {
    t,
    accent,
    heading: t.colors.heading ?? t.colors.text,
    tag: t.plain ? '#ffffff' : t.colors.tag && accent === base.colors.accent ? t.colors.tag : tint(accent, 0.88),
    panel,
    panelText: t.colors.panelText ?? t.colors.text,
    panelMuted: t.colors.panelMuted ?? (darkPanel ? '#cbd5e1' : t.colors.muted),
    subtitleItalic: t.fonts.body === 'SourceSerif',
    pageBody: page.height - t.margins[1] - t.margins[3],
    pageWidth: page.width,
  };
}

export function buildDocument(input: LayoutInput): TDocumentDefinitions {
  if (input.kind === 'cover_letter') return buildLetter(input);
  const th = resolveTheme(input);
  const { t } = th;
  const page = PAGE_SIZES[input.pageSize];
  const [ml, mt, mr, mb] = t.margins;
  const fullWidth = page.width - ml - mr;
  const sections = printedSections(input.content);
  const h = input.content.header;

  let content: Content[];
  let band = 0;
  const backgrounds: ((page: number, size: { width: number; height: number }) => Canvas)[] = [];
  if (t.colors.background && t.colors.background.toLowerCase() !== '#ffffff') {
    const color = t.colors.background;
    backgrounds.push((_p, size) => [{ type: 'rect', x: 0, y: 0, w: size.width, h: size.height, color }]);
  }

  if ((t.layout === 'sidebar' || t.layout === 'columns') && t.sidebar && !t.plain) {
    const gutter = t.layout === 'sidebar' ? 30 : 26;
    const sideWidth = t.sidebar.width;
    const mainWidth = fullWidth - sideWidth - gutter;
    const kinds = t.sidebar.kinds;
    const sideSections = sections.filter((s) => kinds.includes(s.kind));
    const mainSections = sections.filter((s) => !kinds.includes(s.kind));
    const panel = t.layout === 'sidebar';
    const sideStyle = t.sidebar.style ?? (panel ? 'tinted' : 'plain');
    const tinted = panel && sideStyle === 'tinted';
    const sideTheme: Theme = tinted ? th : { ...th, panelText: t.colors.text, panelMuted: t.colors.muted, panel: t.colors.background ?? '#ffffff' };
    // A color band cannot bleed across a side panel: the header sits in the main column.
    const headerTheme: Theme = panel && t.header === 'band' ? { ...th, t: { ...t, header: 'left' } } : th;
    const headerResult = header(input, headerTheme, panel ? mainWidth : fullWidth, !panel);
    const contact = panel ? sideContact(input, sideTheme, sideWidth) : [];
    const sideStack: Content[] = [
      ...contact,
      ...sideSections.map((s, i) => section(s, sideTheme, sideWidth, true, i === 0 && contact.length === 0, i, i === sideSections.length - 1)),
    ];
    const mainStack: Content[] = [
      ...(panel ? headerResult.content : []),
      ...mainSections.map((s, i) => section(s, th, mainWidth, false, i === 0, i, i === mainSections.length - 1)),
    ];
    const outlined = sideStyle === 'outlined';
    const side: Column = outlined
      ? {
          width: sideWidth,
          table: { widths: ['*'], body: [[{ stack: sideStack.length ? sideStack : [{ text: '' }] }]] },
          layout: {
            hLineWidth: () => 0.6,
            vLineWidth: () => 0.6,
            hLineColor: () => t.colors.rule,
            vLineColor: () => t.colors.rule,
            paddingLeft: () => 10,
            paddingRight: () => 10,
            paddingTop: () => 10,
            paddingBottom: () => 10,
          },
        }
      : { width: sideWidth, stack: sideStack.length ? sideStack : [{ text: '' }] };
    const main: Column = { width: '*', stack: mainStack.length ? mainStack : [{ text: '' }] };
    const columns: Content = {
      columns: t.sidebar.side === 'left' ? [side, main] : [main, side],
      columnGap: gutter,
    };
    content = panel ? [columns] : [...headerResult.content, columns];
    band = panel ? 0 : headerResult.band;
    if (tinted) {
      const left = t.sidebar.side === 'left';
      const panelWidth = (left ? ml : mr) + sideWidth + gutter / 2;
      const patternColor = mix(th.panel, th.panelText, 0.12);
      const kind = t.pattern;
      backgrounds.push((_p, size) => {
        const x = left ? 0 : size.width - panelWidth;
        return [
          { type: 'rect', x, y: 0, w: panelWidth, h: size.height, color: th.panel },
          ...pattern(kind, x, 0, panelWidth, size.height, patternColor),
        ];
      });
    }
  } else {
    const headerResult = header(input, th, fullWidth);
    band = headerResult.band;
    content = [...headerResult.content, ...sections.map((s, i) => section(s, th, fullWidth, false, i === 0, i, i === sections.length - 1))];
  }
  if (band > 0) {
    const height = band;
    const kind = t.pattern;
    const patternColor = tint(th.accent, 0.16);
    backgrounds.push((p, size) =>
      p === 1 ? [{ type: 'rect', x: 0, y: 0, w: size.width, h: height, color: th.accent }, ...pattern(kind, 0, 0, size.width, height, patternColor)] : [],
    );
  }

  if (content.length === 0 || (sections.length === 0 && !hasText(h.fullName) && contactItems(h, false).length === 0)) {
    content = [
      ...content,
      { text: 'Add your name and a first section to see your CV here.', color: t.colors.muted, fontSize: t.size.body },
    ];
  }

  return {
    ...common(input, th, page, [ml, mt, mr, mb]),
    content,
    background: backgrounds.length
      ? (p: number, size: { width: number; height: number }) => ({ canvas: backgrounds.flatMap((b) => b(p, size)) })
      : undefined,
    pageBreakBefore: (node, nodes) =>
      node.headlineLevel === 1 && nodes.getFollowingNodesOnPage().length === 0,
  };
}

function common(input: LayoutInput, th: Theme, page: { width: number; height: number }, margins: Margin): TDocumentDefinitions {
  const { t } = th;
  const [, , mr, mb] = margins;
  return {
    pageSize: { width: page.width, height: page.height },
    pageMargins: margins,
    info: {
      title: input.name,
      author: input.content.header.fullName.trim() || undefined,
      creator: 'ReMa Portfolio Studio',
    },
    content: [],
    footer: (current: number, count: number): Content =>
      count > 1
        ? {
            text: `${current} / ${count}`,
            alignment: 'right',
            fontSize: 7.5,
            color: t.colors.muted,
            margin: [0, Math.max(8, mb / 2 - 6), mr, 0],
          }
        : { text: '' },
    defaultStyle: {
      font: t.fonts.body,
      fontSize: t.size.body,
      lineHeight: t.lineHeight,
      color: t.colors.text,
    },
  };
}

// ── Cover letters ────────────────────────────────────────────────────

const EMPTY_LETTER: Required<CoverLetter> = {
  recipientName: '',
  recipientTitle: '',
  company: '',
  address: '',
  position: '',
  date: '',
  subject: '',
  greeting: '',
  body: '',
  closing: '',
  signature: '',
};

export function buildLetter(input: LayoutInput): TDocumentDefinitions {
  const th = resolveTheme({ ...input, kind: 'cover_letter' });
  const { t } = th;
  const page = PAGE_SIZES[input.pageSize];
  const [ml, mt, mr, mb] = t.margins;
  const width = page.width - ml - mr;
  const letter: Required<CoverLetter> = { ...EMPTY_LETTER, ...Object.fromEntries(Object.entries(input.letter ?? {}).filter(([, v]) => typeof v === 'string')) };
  const h = input.content.header;
  const headerResult = header(input, th, width);
  const gap = t.gap.section;
  const align = t.justify ? { alignment: 'justify' as const } : {};

  const recipient = [letter.recipientName, letter.recipientTitle, letter.company, ...letter.address.split('\n')]
    .map((l) => l.trim())
    .filter(Boolean);
  const paragraphs = letter.body
    .split(/\n\s*\n/)
    .map((p) => p.trim())
    .filter(Boolean);
  const content: Content[] = [
    ...headerResult.content,
    ...(hasText(letter.date) ? [{ text: letter.date.trim(), color: t.colors.muted, alignment: t.header === 'center' ? 'right' : 'left', margin: [0, 0, 0, gap] } as Content] : []),
    ...(recipient.length
      ? [{ stack: recipient.map((line, i): Content => ({ text: line, bold: i === 0 && hasText(letter.recipientName) })), margin: [0, 0, 0, gap] } as Content]
      : []),
    ...(hasText(letter.subject)
      ? [{ text: inlineRuns(letter.subject.trim()), bold: true, font: t.fonts.heading, fontSize: t.size.title, color: th.heading, margin: [0, 0, 0, gap * 0.8] } as Content]
      : []),
    ...(hasText(letter.greeting) ? [{ text: letter.greeting.trim(), margin: [0, 0, 0, t.gap.entry] } as Content] : []),
    ...paragraphs.map((p): Content => ({ text: p.split('\n').flatMap((line, i) => [...(i > 0 ? [{ text: '\n' }] : []), ...inlineRuns(line, th.accent)]), margin: [0, 0, 0, t.gap.entry + 2], ...align })),
    ...(hasText(letter.closing) ? [{ text: letter.closing.trim(), margin: [0, gap * 0.5, 0, 0] } as Content] : []),
    ...(hasText(letter.signature)
      ? [{ text: letter.signature.trim(), bold: true, margin: [0, hasText(letter.closing) ? 22 : gap * 0.5, 0, 0] } as Content]
      : []),
  ];
  if (!recipient.length && !paragraphs.length && !hasText(h.fullName)) {
    content.push({ text: 'Add your name, the recipient and a first paragraph to see the letter here.', color: t.colors.muted });
  }
  const backgrounds: ((page: number, size: { width: number; height: number }) => Canvas)[] = [];
  if (headerResult.band > 0) {
    const height = headerResult.band;
    const kind = t.pattern;
    const patternColor = tint(th.accent, 0.16);
    backgrounds.push((p, size) =>
      p === 1 ? [{ type: 'rect', x: 0, y: 0, w: size.width, h: height, color: th.accent }, ...pattern(kind, 0, 0, size.width, height, patternColor)] : [],
    );
  }
  return {
    ...common(input, th, page, [ml, mt, mr, mb]),
    content,
    background: backgrounds.length
      ? (p: number, size: { width: number; height: number }) => ({ canvas: backgrounds.flatMap((b) => b(p, size)) })
      : undefined,
  };
}

/** The pdfmake font dictionary (file names inside its virtual file system). */
export const PDF_FONTS = FONT_FAMILIES;
