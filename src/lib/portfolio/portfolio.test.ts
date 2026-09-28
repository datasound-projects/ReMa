/**
 * Portfolio Studio: templates, layout and real PDF output. PDFs are made
 * by pdfmake with the bundled fonts and read back with PDF.js, the same
 * libraries the app uses.
 */
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';

import * as pdfjs from 'pdfjs-dist/legacy/build/pdf.mjs';
import * as pdfmakeModule from 'pdfmake';
import { beforeAll, describe, expect, it } from 'vitest';

import type { PortfolioContent, PortfolioEntry, PortfolioSection } from '../../services/portfolioService';
import { addSection, moveSection, newSection, removeSection, updateSection } from './content';
import { FONT_FAMILIES, FONT_FILES } from './fonts';
import { buildDocument, defaultPageSize, displayUrl, formatDate, inlineRuns, PAGE_SIZES, printedSections, resolveTheme, stripMarkup, type LayoutInput } from './layout';
import { SAMPLE_CONTENT, SAMPLE_LETTER } from './sample';
import { applyStyle, contrastRatio, defaultStyle, isDefaultStyle, PALETTES } from './style';
import { CATEGORIES, CV_TEMPLATES, DEFAULT_LETTER_TEMPLATE_ID, DEFAULT_TEMPLATE_ID, LETTER_TEMPLATES, matchingLetterTemplate, TEMPLATES, templateById } from './templates';

interface PdfMakeNode {
  virtualfs: { writeFileSync(name: string, content: Uint8Array): void };
  setFonts(fonts: unknown): void;
  setUrlAccessPolicy(policy: (url: string) => boolean): void;
  setLocalAccessPolicy(policy: (path: string) => boolean): void;
  createPdf(doc: unknown): { getBuffer(): Promise<Uint8Array> };
}
const pdfmake = ((pdfmakeModule as unknown as { default?: PdfMakeNode }).default ??
  pdfmakeModule) as unknown as PdfMakeNode;

beforeAll(() => {
  const require = createRequire(import.meta.url);
  for (const [name, path] of Object.entries(FONT_FILES)) {
    pdfmake.virtualfs.writeFileSync(name, readFileSync(require.resolve(path)));
  }
  pdfmake.setFonts(FONT_FAMILIES);
  // Documents never load anything from the web or the disk.
  pdfmake.setUrlAccessPolicy(() => false);
  pdfmake.setLocalAccessPolicy(() => false);
});

async function render(input: LayoutInput): Promise<Uint8Array> {
  return new Uint8Array(await pdfmake.createPdf(buildDocument(input)).getBuffer());
}

interface PageText {
  width: number;
  height: number;
  items: { str: string; x: number; y: number; w: number }[];
}

async function readPdf(bytes: Uint8Array): Promise<PageText[]> {
  const task = pdfjs.getDocument({ data: bytes.slice(), useSystemFonts: false });
  const pdf = await task.promise;
  const pages: PageText[] = [];
  for (let n = 1; n <= pdf.numPages; n++) {
    const page = await pdf.getPage(n);
    const viewport = page.getViewport({ scale: 1 });
    const content = await page.getTextContent();
    pages.push({
      width: viewport.width,
      height: viewport.height,
      items: content.items.flatMap((item) =>
        'str' in item && item.str.trim() ? [{ str: item.str, x: item.transform[4], y: item.transform[5], w: item.width }] : [],
      ),
    });
  }
  await task.destroy();
  return pages;
}

const allText = (pages: PageText[]) =>
  pages
    .flatMap((p) => p.items.map((i) => i.str))
    .join(' ')
    .replace(/ /g, ' ')
    .replace(/\s+/g, ' ');

const entry = (patch: Partial<PortfolioEntry>): PortfolioEntry => ({
  id: Math.random().toString(16).slice(2, 10),
  title: '',
  subtitle: '',
  location: '',
  start: '',
  end: '',
  url: '',
  description: '',
  tags: [],
  ...patch,
});

/** Sample content plus every other kind of section, and a hidden one. */
function fullContent(): PortfolioContent {
  const extra: PortfolioSection[] = [
    {
      ...newSection('projects'),
      entries: [
        entry({
          title: 'Open Data Toolkit',
          subtitle: 'Maintainer',
          start: '2020',
          end: '2024',
          url: 'https://example.org/toolkit',
          description: 'Library for cleaning public datasets.',
          tags: ['Python', 'Pandas'],
        }),
      ],
    },
    {
      ...newSection('certifications'),
      entries: [entry({ title: 'AWS Solutions Architect', subtitle: 'Amazon Web Services', end: '2023-05' })],
    },
    { ...newSection('links'), entries: [entry({ title: 'Blog', url: 'https://blog.example.org/' })] },
    {
      ...newSection('publications'),
      entries: [entry({ title: 'Deep Learning for Tabular Data', subtitle: 'Journal of Data Systems', end: '2023', url: 'https://doi.example.org/1' })],
    },
    { ...newSection('custom'), title: 'Volunteering', text: 'Mentor at a coding school for refugees.' },
    { ...newSection('custom'), title: 'Secret hobbies', text: 'This hidden text must not be printed.', visible: false },
  ];
  return { ...SAMPLE_CONTENT, sections: [...SAMPLE_CONTENT.sections, ...extra] };
}

const input = (templateId: string, content = fullContent(), pageSize: 'a4' | 'letter' = 'a4'): LayoutInput => ({
  name: 'Test CV',
  kind: templateById(templateId).kind === 'letter' ? 'cover_letter' : 'cv',
  templateId,
  pageSize,
  accent: '',
  content,
  letter: SAMPLE_LETTER,
});

describe('bundled fonts', () => {
  it('lay out every character the templates print', async () => {
    const require = createRequire(import.meta.url);
    const fontkit = require(require.resolve('fontkit', { paths: [require.resolve('pdfkit', { paths: [require.resolve('pdfmake')] })] })) as {
      create(buffer: Buffer): { layout(text: string): { glyphs: { advanceWidth: number }[] } };
    };
    for (const [name, path] of Object.entries(FONT_FILES)) {
      const font = fontkit.create(readFileSync(require.resolve(path)));
      const run = font.layout('Alex Morgan // experience · – • ’ “” … | 0123456789 \u00a0Python\u00a0');
      expect(run.glyphs.every((g) => g.advanceWidth >= 0), name).toBe(true);
    }
  });
});

describe('template registry', () => {
  it('has 20 CV templates, two per category, and 5 cover letter templates', () => {
    expect(CV_TEMPLATES).toHaveLength(20);
    expect(LETTER_TEMPLATES).toHaveLength(5);
    const ids = TEMPLATES.map((t) => t.id);
    expect(new Set(ids).size).toBe(ids.length);
    for (const category of CATEGORIES) {
      expect(CV_TEMPLATES.filter((t) => t.category === category).map((t) => t.name), category).toHaveLength(2);
    }
    const names = TEMPLATES.map((t) => t.name);
    for (const name of [
      'Technical Grid',
      'Systems Minimal',
      'Editorial Portfolio',
      'Studio Accent',
      'Structured Professional',
      'Operations Clear',
      'Precision Classic',
      'Finance Modern',
      'Campaign Modern',
      'Brand Story',
      'Results Focus',
      'Relationship Professional',
      'Clinical Clear',
      'Care Professional',
      'Academic Profile',
      'Teaching Modern',
      'Formal Counsel',
      'Administrative Essential',
      'Executive Brief',
      'Leadership Profile',
      'Classic Professional',
      'Modern Minimal',
      'Executive',
      'Creative Editorial',
      'Graduate / Career Change',
    ]) {
      expect(names).toContain(name);
    }
    // Genuinely different: no two templates share layout, header, section and entry treatment.
    const signature = (t: (typeof TEMPLATES)[number]) =>
      JSON.stringify([t.kind, t.layout, t.sidebar?.side, t.header, t.section, t.entry, t.skills, t.fonts, t.dividers, t.pattern]);
    expect(new Set(TEMPLATES.map(signature)).size).toBe(TEMPLATES.length);
    // The requested variety of layouts.
    const tags = new Set(TEMPLATES.flatMap((t) => t.tags));
    for (const tag of ['single-column', 'two-column', 'sidebar', 'minimal', 'editorial', 'executive', 'text-first']) expect(tags.has(tag as never), tag).toBe(true);
    expect(TEMPLATES.every((t) => t.tags.length > 0 && t.description.length > 20)).toBe(true);
  });

  it('falls back to the default template for unknown ids and maps old ids', () => {
    expect(templateById('does-not-exist').id).toBe(DEFAULT_TEMPLATE_ID);
    expect(templateById('does-not-exist', 'letter').id).toBe(DEFAULT_LETTER_TEMPLATE_ID);
    expect(templateById('modern').id).toBe('campaign-modern');
    expect(templateById('ats').plain).toBe(true);
    expect(templateById('data-ai').layout).toBe('sidebar');
    expect(matchingLetterTemplate('executive-brief').id).toBe('letter-executive');
    expect(matchingLetterTemplate('studio-accent').id).toBe('letter-editorial');
    expect(matchingLetterTemplate('campaign-modern').id).toBe('letter-minimal');
  });
});

describe('style customization', () => {
  it('applies palettes, fonts, spacing and layout on top of a template, and resets cleanly', () => {
    const base = templateById('campaign-modern');
    expect(isDefaultStyle(defaultStyle())).toBe(true);
    expect(applyStyle(base, defaultStyle(), '')).toEqual(base);
    const styled = applyStyle(
      base,
      { ...defaultStyle(), palette: 'wine', fontPairing: 'source-serif', scale: 1.08, margins: 'wide', lineSpacing: 'relaxed', sectionSpacing: 'tight', columns: 'sidebar_left', pattern: 'dots', dividers: 'thick', header: 'center', textAlign: 'justify' },
      '',
    );
    expect(styled.colors.accent).toBe('#9f1239');
    expect(styled.fonts.body).toBe('SourceSerif');
    expect(styled.size.body).toBeCloseTo(base.size.body * 1.08, 1);
    expect(styled.margins[0]).toBe(Math.round(base.margins[0] * 1.25));
    expect(styled.lineHeight).toBeGreaterThan(base.lineHeight);
    expect(styled.gap.section).toBeLessThan(base.gap.section);
    expect(styled.layout).toBe('sidebar');
    expect(styled.sidebar?.side).toBe('left');
    expect(styled.pattern).toBe('dots');
    expect(styled.dividers).toBe('thick');
    expect(styled.header).toBe('center');
    expect(styled.justify).toBe(true);
    // The base template is untouched.
    expect(base.layout).toBe('single');
    // A chosen accent wins over the palette's.
    expect(applyStyle(base, { ...defaultStyle(), palette: 'wine' }, '#0f766e').colors.accent).toBe('#0f766e');
  });

  it('keeps text readable: unreadable custom colors fall back, panels stay in contrast', () => {
    const base = templateById('campaign-modern');
    const pale = applyStyle(base, { ...defaultStyle(), textColor: '#f0f0f0', headingColor: '#ffffff', panelColor: '#1e3a5f' }, '');
    expect(pale.colors.text).toBe(base.colors.text);
    expect(pale.colors.heading ?? pale.colors.text).toBe(base.colors.heading ?? base.colors.text);
    expect(pale.colors.panelText).toBe('#ffffff');
    for (const p of PALETTES) {
      expect(contrastRatio(p.text, '#ffffff'), p.name).toBeGreaterThan(4.5);
      expect(contrastRatio(p.panelText, p.panel), p.name).toBeGreaterThan(4.5);
    }
    // Text-first templates ignore color choices.
    const plain = applyStyle(templateById('administrative-essential'), { ...defaultStyle(), palette: 'wine' }, '#9f1239');
    expect(plain.colors.accent).toBe('#000000');
  });

  it('inline markup: bold, italic and links', () => {
    const runs = inlineRuns('Built **fast** pipelines with _care_ and [docs](https://example.org/docs).', '#0a66c2');
    expect(runs).toEqual([
      { text: 'Built ' },
      { text: 'fast', bold: true },
      { text: ' pipelines with ' },
      { text: 'care', italics: true },
      { text: ' and ' },
      { text: 'docs', link: 'https://example.org/docs', color: '#0a66c2' },
      { text: '.' },
    ]);
    expect(stripMarkup('a **b** [c](https://x.y)')).toBe('a b c');
    expect(inlineRuns('snake_case_name and 2*3*4')).toEqual([{ text: 'snake_case_name and 2*3*4' }]);
  });
});

describe('layout', () => {
  it('formats dates and links for print', () => {
    expect(formatDate('2021-03')).toBe('Mar 2021');
    expect(formatDate('Present')).toBe('Present');
    expect(displayUrl('https://www.linkedin.com/in/ana/')).toBe('linkedin.com/in/ana');
    expect(defaultPageSize('en-US')).toBe('letter');
    expect(defaultPageSize('de-AT')).toBe('a4');
  });

  it('uses the chosen page size', () => {
    const a4 = buildDocument(input('modern', fullContent(), 'a4')).pageSize as { width: number; height: number };
    const letter = buildDocument(input('modern', fullContent(), 'letter')).pageSize as { width: number; height: number };
    expect(a4).toEqual({ width: PAGE_SIZES.a4.width, height: PAGE_SIZES.a4.height });
    expect(letter).toEqual({ width: 612, height: 792 });
  });

  it('leaves out hidden and empty sections', () => {
    const content = fullContent();
    content.sections.push({ ...newSection('languages'), entries: [] });
    const printed = printedSections(content).map((s) => s.title);
    expect(printed).not.toContain('Secret hobbies');
    expect(printed.filter((t) => t === 'Languages')).toHaveLength(1);
  });

  it('never changes the content (switching templates keeps everything)', () => {
    const content = fullContent();
    const before = JSON.stringify(content);
    for (const t of TEMPLATES) buildDocument(input(t.id, content));
    expect(JSON.stringify(content)).toBe(before);
  });

  it('resolves a theme for every template with and without a custom style', () => {
    for (const t of TEMPLATES) {
      const plain = resolveTheme({ templateId: t.id, accent: '', pageSize: 'a4', kind: t.kind === 'letter' ? 'cover_letter' : 'cv' });
      expect(plain.t.id).toBe(t.id);
      const styled = resolveTheme({ templateId: t.id, accent: '#6d28d9', pageSize: 'a4', style: { ...defaultStyle(), palette: 'midnight', columns: 'sidebar_right' } });
      if (!t.plain) expect(styled.accent).toBe('#6d28d9');
      expect(contrastRatio(styled.panelText, styled.panel), t.id).toBeGreaterThan(3);
    }
  });
});

describe('editing helpers', () => {
  it('adds, moves, updates and removes sections without touching others', () => {
    let content: PortfolioContent = { header: SAMPLE_CONTENT.header, sections: [] };
    content = addSection(content, 'summary');
    content = addSection(content, 'experience');
    const [summary, experience] = content.sections;
    expect(summary?.kind).toBe('summary');
    expect(experience?.entries).toHaveLength(1);
    content = moveSection(content, experience?.id ?? '', -1);
    expect(content.sections.map((s) => s.kind)).toEqual(['experience', 'summary']);
    content = updateSection(content, summary?.id ?? '', (s) => ({ ...s, visible: false }));
    expect(content.sections[1]?.visible).toBe(false);
    content = removeSection(content, experience?.id ?? '');
    expect(content.sections.map((s) => s.kind)).toEqual(['summary']);
  });
});

describe('PDF export', () => {
  it.each(TEMPLATES.map((t) => [t.name, t.id]))(
    '%s: real, selectable text inside the page margins, same content',
    async (_name, id) => {
      const template = templateById(id);
      const isLetter = template.kind === 'letter';
      for (const size of ['a4', 'letter'] as const) {
        const bytes = await render(input(id, fullContent(), size));
        expect(new TextDecoder().decode(bytes.slice(0, 5))).toBe('%PDF-');
        const pages = await readPdf(bytes);
        expect(pages.length).toBeGreaterThanOrEqual(1);
        const text = allText(pages);
        const expectedCv = [
          'Alex Morgan',
          'Senior Data Engineer',
          'Northwind Analytics',
          'Contoso Retail',
          'TU Wien',
          'Open Data Toolkit',
          'AWS Solutions Architect',
          'Mentor at a coding school for refugees.',
          'Kafka',
          'German',
          'Deep Learning for Tabular Data',
        ];
        const expectedLetter = ['Alex Morgan', 'Jordan Lee', 'Fabrikam', 'Application for Staff Data Engineer', 'Kind regards,', 'two billion events'];
        for (const expected of isLetter ? expectedLetter : expectedCv) {
          expect(text, `${id}/${size}`).toContain(expected);
        }
        expect(text).not.toContain('This hidden text must not be printed.');
        const [ml, , mr] = template.margins;
        for (const page of pages) {
          expect(Math.abs(page.width - PAGE_SIZES[size].width)).toBeLessThan(1);
          for (const item of page.items) {
            // Nothing is cut off, and text keeps the template's side margins.
            expect(item.x, `${id}/${size}: "${item.str}"`).toBeGreaterThanOrEqual(ml - 1.5);
            expect(item.x + item.w, `${id}/${size}: "${item.str}"`).toBeLessThanOrEqual(page.width - mr + 1.5);
            expect(item.y).toBeGreaterThan(0);
            expect(item.y).toBeLessThan(page.height);
          }
        }
      }
    },
    30_000,
  );

  it('long CVs flow onto more pages without losing anything', async () => {
    const long: PortfolioContent = {
      header: SAMPLE_CONTENT.header,
      sections: [
        {
          ...newSection('experience'),
          entries: Array.from({ length: 24 }, (_, i) =>
            entry({
              title: `Role number ${i + 1}`,
              subtitle: `Company ${i + 1}`,
              start: '2010-01',
              end: '2011-01',
              description: Array.from({ length: 6 }, (_, j) => `- Achievement ${i + 1}.${j + 1} with measurable results`).join('\n'),
            }),
          ),
        },
        // One entry far longer than a page must still print completely.
        {
          ...newSection('custom'),
          title: 'Publications',
          entries: [
            entry({
              title: 'Collected papers',
              description: Array.from({ length: 140 }, (_, j) => `- Paper ${j + 1}: a study on data systems`).join('\n'),
            }),
          ],
        },
      ],
    };
    for (const id of ['campaign-modern', 'technical-grid', 'academic-profile', 'administrative-essential', 'brand-story', 'leadership-profile']) {
      const pages = await readPdf(await render(input(id, long)));
      expect(pages.length).toBeGreaterThan(2);
      const text = allText(pages);
      for (let i = 1; i <= 24; i++) expect(text, id).toContain(`Role number ${i}`);
      expect(text).toContain('Paper 1:');
      expect(text).toContain('Paper 140:');
      expect(text).toContain(`${pages.length} / ${pages.length}`);
    }
  }, 90_000);

  it('short and nearly empty documents render on one page in every template', async () => {
    const short: PortfolioContent = {
      header: { ...SAMPLE_CONTENT.header, headline: '', website: '', linkedin: '', github: '' },
      sections: [{ ...newSection('summary'), text: 'Short **summary** with a [link](https://example.org).' }],
    };
    for (const t of TEMPLATES) {
      const pages = await readPdf(await render(input(t.id, short)));
      expect(pages.length, t.id).toBe(1);
      const text = allText(pages);
      expect(text).toContain('Alex Morgan');
      if (t.kind === 'cv') {
        expect(text).toContain('summary');
        expect(text).not.toContain('**');
      }
    }
    const empty = await readPdf(await render(input('campaign-modern', { header: { ...SAMPLE_CONTENT.header, fullName: '', email: '', phone: '', location: '', website: '', linkedin: '', github: '', headline: '' }, sections: [] })));
    expect(allText(empty)).toContain('Add your name');
  }, 90_000);

  it('a custom style renders for every template (band, patterns, panels, photo)', async () => {
    const pixel =
      'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==';
    const content = fullContent();
    content.header.photo = pixel;
    for (const t of TEMPLATES) {
      const styled: LayoutInput = {
        ...input(t.id, content),
        accent: '#6d28d9',
        style: { ...defaultStyle(), palette: 'plum', pattern: 'diagonal', dividers: 'dotted', header: t.kind === 'letter' ? 'band' : 'band', columns: t.kind === 'cv' ? 'sidebar_left' : '', showPhoto: true, fontPairing: 'playfair-sans', scale: 0.9 },
      };
      const pages = await readPdf(await render(styled));
      expect(allText(pages), t.id).toContain('Alex Morgan');
    }
  }, 120_000);
});
