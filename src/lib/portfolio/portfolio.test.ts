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
import { buildDocument, defaultPageSize, displayUrl, formatDate, PAGE_SIZES, printedSections, type LayoutInput } from './layout';
import { SAMPLE_CONTENT } from './sample';
import { DEFAULT_TEMPLATE_ID, TEMPLATES, templateById } from './templates';

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
    { ...newSection('custom'), title: 'Volunteering', text: 'Mentor at a coding school for refugees.' },
    { ...newSection('custom'), title: 'Secret hobbies', text: 'This hidden text must not be printed.', visible: false },
  ];
  return { ...SAMPLE_CONTENT, sections: [...SAMPLE_CONTENT.sections, ...extra] };
}

const input = (templateId: string, content = fullContent(), pageSize: 'a4' | 'letter' = 'a4'): LayoutInput => ({
  name: 'Test CV',
  templateId,
  pageSize,
  accent: '',
  content,
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
  it('has at least 12 distinct templates, including the requested designs', () => {
    expect(TEMPLATES.length).toBeGreaterThanOrEqual(12);
    const ids = TEMPLATES.map((t) => t.id);
    expect(new Set(ids).size).toBe(ids.length);
    const names = TEMPLATES.map((t) => t.name);
    for (const name of [
      'Minimal',
      'Modern',
      'Executive',
      'Technical',
      'Data / AI',
      'Consulting',
      'Product',
      'Creative',
      'Academic',
      'Compact',
      'Two-column',
      'ATS-friendly',
    ]) {
      expect(names).toContain(name);
    }
    // Genuinely different: no two templates share every design choice.
    const signature = (t: (typeof TEMPLATES)[number]) =>
      JSON.stringify([t.layout, t.header, t.section, t.entry, t.skills, t.fonts, t.colors.accent]);
    expect(new Set(TEMPLATES.map(signature)).size).toBe(TEMPLATES.length);
  });

  it('falls back to the default template for unknown ids', () => {
    expect(templateById('does-not-exist').id).toBe(DEFAULT_TEMPLATE_ID);
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
      for (const size of ['a4', 'letter'] as const) {
        const bytes = await render(input(id, fullContent(), size));
        expect(new TextDecoder().decode(bytes.slice(0, 5))).toBe('%PDF-');
        const pages = await readPdf(bytes);
        expect(pages.length).toBeGreaterThanOrEqual(1);
        const text = allText(pages);
        for (const expected of [
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
        ]) {
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
    for (const id of ['modern', 'data-ai', 'academic', 'ats']) {
      const pages = await readPdf(await render(input(id, long)));
      expect(pages.length).toBeGreaterThan(2);
      const text = allText(pages);
      for (let i = 1; i <= 24; i++) expect(text, id).toContain(`Role number ${i}`);
      expect(text).toContain('Paper 1:');
      expect(text).toContain('Paper 140:');
      expect(text).toContain(`${pages.length} / ${pages.length}`);
    }
  }, 60_000);
});
