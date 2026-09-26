/**
 * The layout engine: turns a Portfolio Studio document and a template into
 * a pdfmake document definition. Pure (no pdfmake runtime), so the preview,
 * the export and the tests all use exactly the same layout.
 */
import type { Column, Content, ContentText, TDocumentDefinitions } from 'pdfmake/interfaces';

import type {
  PageSize,
  PortfolioContent,
  PortfolioEntry,
  PortfolioHeader,
  PortfolioSection,
  SectionKind,
} from '../../services/portfolioService';
import { FONT_FAMILIES } from './fonts';
import { templateById, type Template } from './templates';

/** Page sizes in points (1/72 inch). */
export const PAGE_SIZES: Record<PageSize, { width: number; height: number; label: string }> = {
  a4: { width: 595.28, height: 841.89, label: 'A4' },
  letter: { width: 612, height: 792, label: 'Letter' },
};

/** Letter where it is the standard paper size, A4 elsewhere. */
export function defaultPageSize(language = typeof navigator === 'undefined' ? 'en' : navigator.language): PageSize {
  const region = (language.split('-')[1] ?? '').toUpperCase();
  return ['US', 'CA', 'MX', 'PH', 'CL', 'CO', 'VE'].includes(region) ? 'letter' : 'a4';
}

export interface LayoutInput {
  name: string;
  templateId: string;
  pageSize: PageSize;
  /** `#rrggbb`, or empty for the template's own accent. */
  accent: string;
  content: PortfolioContent;
}

/** Everything the builders need about the current design. */
interface Theme {
  t: Template;
  accent: string;
  tag: string;
  panel: string;
  subtitleItalic: boolean;
  /** Height of the page's writable area (for keep-together decisions). */
  pageBody: number;
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
  const match = /^#?([0-9a-f]{6})$/i.exec(hex.trim());
  if (!match) return '#f2f2f2';
  const n = parseInt(match[1] ?? '000000', 16);
  const channel = (shift: number) => {
    const c = (n >> shift) & 0xff;
    return Math.round(c + (255 - c) * amount)
      .toString(16)
      .padStart(2, '0');
  };
  return `#${channel(16)}${channel(8)}${channel(0)}`;
}

/** Non-breaking spaces keep a short tag on one line. */
const chipText = (tag: string) => {
  const t = tag.trim();
  const inner = t.length <= 22 ? t.replace(/ /g, ' ') : t;
  return ` ${inner} `;
};

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
  let paragraph: string[] = [];
  let bullets: string[] = [];
  const flushParagraph = () => {
    if (paragraph.length) out.push({ text: paragraph.join('\n'), color, margin: [0, 2, 0, 0] });
    paragraph = [];
  };
  const flushBullets = () => {
    if (bullets.length) {
      out.push({
        ul: bullets.map((b) => ({ text: b, color })),
        markerColor: th.t.ats ? th.t.colors.text : th.accent,
        margin: [2, 2, 0, 0],
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

const rule = (width: number, color: string, weight = 0.6, margin: [number, number, number, number] = [0, 2, 0, 7]): Content => ({
  canvas: [{ type: 'line', x1: 0, y1: 0, x2: width, y2: 0, lineWidth: weight, lineColor: color }],
  margin,
});

function sectionTitle(title: string, th: Theme, width: number, side = false): Content {
  const { t } = th;
  const heading = t.fonts.heading;
  const style = side && t.section !== 'rule' && t.section !== 'plain' ? 'caps' : t.section;
  const upper = title.toUpperCase();
  switch (style) {
    case 'rule':
      return {
        stack: [
          { text: upper, font: heading, fontSize: t.size.section, color: t.colors.text, characterSpacing: 1.1 },
          rule(width, t.colors.rule),
        ],
      };
    case 'caps':
      return {
        text: upper,
        font: heading,
        fontSize: t.size.section,
        color: th.accent,
        characterSpacing: 1.2,
        margin: [0, 0, 0, 6],
      };
    case 'bar':
      return {
        columns: [
          {
            width: 9,
            canvas: [{ type: 'rect', x: 0, y: 1.5, w: 3, h: t.size.section * 0.95, r: 1.5, color: th.accent }],
          },
          { width: '*', text: title, font: heading, fontSize: t.size.section, color: t.colors.text },
        ],
        margin: [0, 0, 0, 7],
      };
    case 'underline':
      return {
        stack: [
          { text: title, font: heading, fontSize: t.size.section, color: t.colors.text },
          rule(26, th.accent, 2, [0, 3, 0, 8]),
        ],
      };
    case 'mono':
      return {
        stack: [
          {
            text: [
              { text: '// ', color: tint(th.accent, 0.35) },
              { text: title.toLowerCase(), color: th.accent },
            ],
            font: t.fonts.label ?? heading,
            fontSize: t.size.section,
            bold: true,
          },
          rule(width, t.colors.rule, 0.5, [0, 3, 0, 7]),
        ],
      };
    case 'serif':
      return {
        text: title,
        font: heading,
        fontSize: t.size.section,
        italics: true,
        color: th.accent,
        margin: [0, 0, 0, 6],
      };
    case 'label':
      return {
        text: upper,
        font: heading,
        fontSize: t.size.section,
        color: th.accent,
        characterSpacing: 0.9,
        lineHeight: 1.2,
      };
    case 'plain':
      return {
        text: upper,
        font: heading,
        bold: true,
        fontSize: t.size.section,
        color: t.colors.text,
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
    ...(e.url.trim() && !th.t.ats ? { link: e.url.trim() } : {}),
  };
}

function subtitleLine(parts: string[], th: Theme, color = th.t.colors.muted): Content | null {
  const text = parts.map((p) => p.trim()).filter(Boolean).join(' · ');
  if (!text) return null;
  return { text, color, italics: th.subtitleItalic, fontSize: th.t.size.body, margin: [0, 1, 0, 0] };
}

function tagsLine(tags: string[], th: Theme, label: string | null): Content | null {
  const clean = tags.map((t) => t.trim()).filter(Boolean);
  if (clean.length === 0) return null;
  if (th.t.skills === 'tags' && !th.t.ats) {
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
    color: th.t.colors.muted,
    margin: [0, 2, 0, 0],
  };
}

/** Experience, education, projects, certifications and custom entries. */
function timelineEntry(e: PortfolioEntry, kind: SectionKind, th: Theme, width: number): Content {
  const { t } = th;
  const dates = period(e);
  const hasTitle = hasText(e.title);
  const subParts = [hasTitle ? e.subtitle : '', e.location];
  const body: Content[] = [
    ...richText(e.description, th),
    ...(kind === 'projects' ? [tagsLine(e.tags, th, 'Tools')] : [tagsLine(e.tags, th, null)]).filter(
      (c): c is Content => c !== null,
    ),
  ];
  const url =
    e.url.trim() && (t.ats || kind === 'certifications')
      ? [{ text: t.ats ? e.url.trim() : displayUrl(e.url), link: e.url.trim(), color: t.colors.muted, fontSize: t.size.small }]
      : [];

  let stack: Content[];
  if (t.entry === 'dates-left' && !t.ats) {
    const dateWidth = 74;
    stack = [
      {
        columns: [
          { width: dateWidth, text: dates, fontSize: t.size.small, color: t.colors.muted, margin: [0, 1, 0, 0] },
          {
            width: '*',
            stack: [titleText(e, th), subtitleLine(subParts, th), ...body, ...url].filter(
              (c): c is Content => c !== null,
            ),
          },
        ],
        columnGap: 12,
      },
    ];
  } else if (t.entry === 'dates-right' && !t.ats && dates) {
    stack = [
      {
        columns: [
          { width: '*', ...titleText(e, th) },
          { width: 'auto', text: dates, fontSize: t.size.small, color: t.colors.muted, margin: [10, 1.5, 0, 0] },
        ],
      },
      subtitleLine(subParts, th),
      ...body,
      ...url,
    ].filter((c): c is Content => c !== null);
  } else {
    stack = [titleText(e, th), subtitleLine([...subParts, dates], th), ...body, ...url].filter(
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
    const margin: [number, number, number, number] = [0, 0, 0, i < groups.length - 1 ? 6 : 0];
    if (side && t.skills === 'list') {
      return {
        stack: [
          ...(label ? [{ text: label, bold: true, fontSize: t.size.small, color: th.accent, margin: [0, 0, 0, 2] } as Content] : []),
          ...tags.map((tag): Content => ({ text: tag, fontSize: t.size.body, margin: [0, 0, 0, 1.5] })),
        ],
        margin,
      };
    }
    if (t.skills === 'tags' && !t.ats) {
      return {
        stack: [
          ...(label ? [{ text: label, bold: true, fontSize: t.size.small, color: t.colors.muted } as Content] : []),
          tagsLine(tags, th, null) ?? { text: '' },
        ],
        margin,
      };
    }
    return {
      text: [...(label ? [{ text: `${label}: `, bold: true }] : []), { text: tags.join(t.ats ? ', ' : ' · ') }],
      margin,
    };
  });
}

function languageLines(entries: PortfolioEntry[], th: Theme, side: boolean): Content[] {
  const { t } = th;
  const items = entries.filter((e) => hasText(e.title));
  if (!side) {
    return [
      {
        text: items.flatMap((e, i) => [
          ...(i > 0 ? [{ text: t.ats ? ', ' : '  ·  ', color: t.colors.muted }] : []),
          { text: e.title.trim(), bold: true },
          ...(hasText(e.subtitle) ? [{ text: ` (${e.subtitle.trim()})`, color: t.colors.muted }] : []),
        ]),
      },
    ];
  }
  return items.map(
    (e): Content => ({
      text: [
        { text: e.title.trim(), bold: true },
        ...(hasText(e.subtitle) ? [{ text: `  ${e.subtitle.trim()}`, color: t.colors.muted }] : []),
      ],
      margin: [0, 0, 0, 2],
    }),
  );
}

function linkLines(entries: PortfolioEntry[], th: Theme, side: boolean): Content[] {
  const { t } = th;
  return entries
    .filter((e) => hasText(e.url) || hasText(e.title))
    .map((e): Content => {
      const label = e.title.trim();
      const url = e.url.trim();
      const shown = t.ats ? url : displayUrl(url);
      if (side) {
        return {
          stack: [
            ...(label ? [{ text: label, bold: true, fontSize: t.size.small } as Content] : []),
            ...(url ? [{ text: shown, link: url, color: th.accent, fontSize: t.size.small, wordBreak: 'break-all' } as Content] : []),
          ],
          margin: [0, 0, 0, 4],
        };
      }
      return {
        text: [
          ...(label ? [{ text: url ? `${label}: ` : label, bold: true }] : []),
          ...(url ? [{ text: shown, link: url, color: t.ats ? t.colors.text : th.accent }] : []),
        ],
        margin: [0, 0, 0, 2],
      };
    });
}

function certificationLines(entries: PortfolioEntry[], th: Theme): Content[] {
  const { t } = th;
  return entries.filter(entryHasContent).map(
    (e): Content => ({
      stack: [
        { text: e.title.trim() || e.subtitle.trim(), bold: true, fontSize: t.size.body, ...(e.url.trim() ? { link: e.url.trim() } : {}) },
        ...[subtitleLine([hasText(e.title) ? e.subtitle : '', period(e)], th, th.t.colors.muted)]
          .filter((c): c is Content => c !== null)
          .map((c) => ({ ...(c as object), fontSize: t.size.small }) as Content),
      ],
      margin: [0, 0, 0, 5],
    }),
  );
}

/** The body of one section (without its title). */
function sectionBody(s: PortfolioSection, th: Theme, width: number, side: boolean): Content[] {
  const entries = s.entries.filter(entryHasContent);
  const text = hasText(s.text) ? richText(s.text, th) : [];
  switch (s.kind) {
    case 'summary':
      return [...text, ...entries.map((e) => timelineEntry(e, s.kind, th, width))];
    case 'skills':
      return [...text, ...skillGroups(entries, th, side)];
    case 'languages':
      return [...text, ...languageLines(entries, th, side)];
    case 'links':
      return [...text, ...linkLines(entries, th, side)];
    case 'certifications':
      return side
        ? [...text, ...certificationLines(entries, th)]
        : [...text, ...entries.map((e) => timelineEntry(e, s.kind, th, width))];
    default:
      return [...text, ...entries.map((e) => timelineEntry(e, s.kind, th, width))];
  }
}

function section(s: PortfolioSection, th: Theme, width: number, side: boolean, first: boolean): Content {
  const { t } = th;
  const margin: [number, number, number, number] = [0, first ? 0 : t.gap.section, 0, 0];
  if (t.section === 'label' && !side) {
    const labelWidth = 92;
    return {
      columns: [
        { width: labelWidth, ...(sectionTitle(s.title, th, labelWidth) as object) } as Content,
        { width: '*', stack: sectionBody(s, th, width - labelWidth - 14, side) },
      ],
      columnGap: 14,
      margin,
    };
  }
  const title = { ...(sectionTitle(s.title, th, width, side) as object), headlineLevel: 1 } as Content;
  const body = sectionBody(s, th, width, side);
  const [firstPart, ...rest] = body;
  // The title always travels with the start of its first entry.
  const head: Content =
    firstPart !== undefined && isSmall(firstPart)
      ? { stack: [title, firstPart], unbreakable: true }
      : { stack: [title, ...(firstPart !== undefined ? [firstPart] : [])] };
  return { stack: [head, ...rest], margin };
}

/** Body parts known to be short (kept together already, or a short paragraph). */
function isSmall(part: Content): boolean {
  if (typeof part === 'object' && part !== null && 'unbreakable' in part) return true;
  if (typeof part === 'object' && part !== null && 'text' in part && typeof part.text === 'string') {
    return part.text.length < 700;
  }
  return false;
}

// ── Header ───────────────────────────────────────────────────────────

interface ContactItem {
  text: string;
  link?: string;
}

function contactItems(h: PortfolioHeader, ats: boolean): ContactItem[] {
  const url = (value: string): ContactItem | null =>
    hasText(value) ? { text: ats ? value.trim() : displayUrl(value), link: value.trim() } : null;
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
  const sep = th.t.ats ? '  |  ' : '   ·   ';
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

function header(h: PortfolioHeader, th: Theme, width: number, withContact = true): Content[] {
  const { t } = th;
  const items = withContact ? contactItems(h, !!t.ats) : [];
  const nameColor = t.accentName ? th.accent : t.colors.text;
  const headlineColor = t.ats || t.id === 'minimal' ? t.colors.muted : th.accent;
  const gap = t.gap.section;
  const clean = (list: (Content | null)[]) => list.filter((c): c is Content => c !== null);

  switch (t.header) {
    case 'center':
      return [
        {
          stack: clean([
            nameText(h, th, nameColor, 'center'),
            headlineText(h, th, headlineColor, 'center'),
            contactLine(items, th, t.colors.muted, t.colors.muted, 'center'),
          ]),
        },
        rule(width, t.colors.rule, 0.8, [0, 10, 0, gap - 4]),
      ];
    case 'split':
      return [
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
        rule(width, th.accent, 1.2, [0, 10, 0, gap - 4]),
      ];
    case 'band': {
      const [left, top, right] = t.margins;
      const soft = tint(th.accent, 0.78);
      return [
        {
          table: {
            widths: ['*'],
            body: [
              [
                {
                  stack: clean([
                    nameText(h, th, '#ffffff'),
                    headlineText(h, th, soft),
                    contactLine(items, th, soft, '#ffffff'),
                  ]),
                  fillColor: th.accent,
                },
              ],
            ],
          },
          layout: {
            hLineWidth: () => 0,
            vLineWidth: () => 0,
            paddingLeft: () => left,
            paddingRight: () => right,
            paddingTop: () => Math.max(26, top - 6),
            paddingBottom: () => 22,
          },
          // Bleeds to the page edges.
          margin: [-left, -top, -right, gap],
        },
      ];
    }
    case 'left':
    default:
      return [
        {
          stack: clean([
            nameText(h, th, nameColor),
            headlineText(h, th, headlineColor),
            contactLine(items, th, t.colors.muted, t.ats ? t.colors.text : t.colors.muted),
          ]),
          margin: [0, 0, 0, gap],
        },
      ];
  }
}

/** Contact details as a side-panel block (sidebar layout). */
function sideContact(h: PortfolioHeader, th: Theme): Content[] {
  const items = contactItems(h, false);
  if (items.length === 0) return [];
  return [
    sectionTitle('Contact', th, 0, true),
    {
      stack: items.map(
        (c): Content => ({
          text: c.text,
          ...(c.link ? { link: c.link, color: th.accent } : {}),
          fontSize: th.t.size.small,
          wordBreak: 'break-all',
          margin: [0, 0, 0, 3],
        }),
      ),
    },
  ];
}

// ── Document ─────────────────────────────────────────────────────────

export function resolveTheme(input: Pick<LayoutInput, 'templateId' | 'accent' | 'pageSize'>): Theme {
  const t = templateById(input.templateId);
  const accent = t.ats ? t.colors.accent : /^#[0-9a-f]{6}$/i.test(input.accent) ? input.accent : t.colors.accent;
  const page = PAGE_SIZES[input.pageSize];
  return {
    t,
    accent,
    tag: t.ats ? '#ffffff' : tint(accent, 0.88),
    panel: t.colors.panel && accent === t.colors.accent ? t.colors.panel : tint(accent, 0.93),
    subtitleItalic: t.fonts.body === 'SourceSerif',
    pageBody: page.height - t.margins[1] - t.margins[3],
  };
}

export function buildDocument(input: LayoutInput): TDocumentDefinitions {
  const th = resolveTheme(input);
  const { t } = th;
  const page = PAGE_SIZES[input.pageSize];
  const [ml, mt, mr, mb] = t.margins;
  const fullWidth = page.width - ml - mr;
  const sections = printedSections(input.content);
  const h = input.content.header;

  let content: Content[];
  let background: TDocumentDefinitions['background'];

  if ((t.layout === 'sidebar' || t.layout === 'columns') && t.sidebar && !t.ats) {
    const gutter = t.layout === 'sidebar' ? 30 : 26;
    const sideWidth = t.sidebar.width;
    const mainWidth = fullWidth - sideWidth - gutter;
    const kinds = t.sidebar.kinds;
    const sideSections = sections.filter((s) => kinds.includes(s.kind));
    const mainSections = sections.filter((s) => !kinds.includes(s.kind));
    const panel = t.layout === 'sidebar';
    const sideStack: Content[] = [
      ...(panel ? sideContact(h, th) : []),
      ...sideSections.map((s, i) =>
        section(s, th, sideWidth, true, i === 0 && !(panel && contactItems(h, false).length > 0)),
      ),
    ];
    const mainStack: Content[] = [
      ...(panel ? header(h, th, mainWidth, false) : []),
      ...mainSections.map((s, i) => section(s, th, mainWidth, false, i === 0)),
    ];
    const side: Column = { width: sideWidth, stack: sideStack.length ? sideStack : [{ text: '' }] };
    const main: Column = { width: '*', stack: mainStack.length ? mainStack : [{ text: '' }] };
    const columns: Content = {
      columns: t.sidebar.side === 'left' ? [side, main] : [main, side],
      columnGap: gutter,
    };
    content = panel ? [columns] : [...header(h, th, fullWidth), columns];
    if (panel) {
      const panelWidth = (t.sidebar.side === 'left' ? ml : mr) + sideWidth + gutter / 2;
      background = (_page: number, size: { width: number; height: number }) => ({
        canvas: [
          {
            type: 'rect',
            x: t.sidebar?.side === 'left' ? 0 : size.width - panelWidth,
            y: 0,
            w: panelWidth,
            h: size.height,
            color: th.panel,
          },
        ],
      });
    }
  } else {
    content = [...header(h, th, fullWidth), ...sections.map((s, i) => section(s, th, fullWidth, false, i === 0))];
  }

  if (content.length === 0 || (sections.length === 0 && !hasText(h.fullName) && contactItems(h, false).length === 0)) {
    content = [
      ...content,
      { text: 'Add your name and a first section to see your CV here.', color: t.colors.muted, fontSize: t.size.body },
    ];
  }

  return {
    pageSize: { width: page.width, height: page.height },
    pageMargins: [ml, mt, mr, mb],
    info: {
      title: input.name,
      author: h.fullName.trim() || undefined,
      creator: 'ReMa Portfolio Studio',
    },
    content,
    background,
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
    pageBreakBefore: (node, nodes) =>
      node.headlineLevel === 1 && nodes.getFollowingNodesOnPage().length === 0,
    defaultStyle: {
      font: t.fonts.body,
      fontSize: t.size.body,
      lineHeight: t.lineHeight,
      color: t.colors.text,
    },
  };
}

/** The pdfmake font dictionary (file names inside its virtual file system). */
export const PDF_FONTS = FONT_FAMILIES;
