/**
 * Portfolio Studio templates: a registry of designs. A template is data —
 * layout, type, color and spacing choices — read by one layout engine
 * (`layout.ts`), so adding a design means adding an entry here. Content
 * never depends on the template: switching keeps everything.
 */
import type { SectionKind } from '../../services/portfolioService';
import type { FontName } from './fonts';

export type HeaderStyle = 'left' | 'center' | 'band' | 'split';
/** How section titles look. */
export type SectionStyle = 'rule' | 'caps' | 'bar' | 'underline' | 'mono' | 'serif' | 'label' | 'plain';
/** Where dates go in an entry. */
export type EntryStyle = 'stacked' | 'dates-right' | 'dates-left';
export type SkillStyle = 'inline' | 'tags' | 'list';

export interface Template {
  id: string;
  name: string;
  /** One line for the gallery. */
  description: string;
  /** Page structure: one column, a tinted side panel, or two plain columns. */
  layout: 'single' | 'sidebar' | 'columns';
  sidebar?: {
    side: 'left' | 'right';
    /** Points. */
    width: number;
    /** Section kinds placed in the side column. */
    kinds: SectionKind[];
  };
  fonts: { body: FontName; heading: FontName; name: FontName; label?: FontName };
  colors: {
    /** Default accent; the user can pick another. */
    accent: string;
    text: string;
    muted: string;
    rule: string;
    /** Side panel background and text (sidebar layout). */
    panel?: string;
    panelText?: string;
    panelMuted?: string;
    /** Tag backgrounds (skill chips). */
    tag?: string;
  };
  /** Font sizes in points. */
  size: { body: number; small: number; name: number; headline: number; section: number; title: number };
  lineHeight: number;
  /** Page margins [left, top, right, bottom] in points. */
  margins: [number, number, number, number];
  header: HeaderStyle;
  section: SectionStyle;
  entry: EntryStyle;
  skills: SkillStyle;
  /** Space before a section and between entries, in points. */
  gap: { section: number; entry: number };
  /** Name in the accent color. */
  accentName?: boolean;
  /** Plain text only (no graphics, columns or color) for applicant tracking systems. */
  ats?: boolean;
}

const SIDE_KINDS: SectionKind[] = ['skills', 'languages', 'links', 'certifications'];

export const TEMPLATES: readonly Template[] = [
  {
    id: 'minimal',
    name: 'Minimal',
    description: 'Quiet and precise: black type, fine rules, lots of air.',
    layout: 'single',
    fonts: { body: 'Inter', heading: 'InterSemi', name: 'InterSemi' },
    colors: { accent: '#374151', text: '#1f2328', muted: '#6b7280', rule: '#d1d5db', tag: '#f3f4f6' },
    size: { body: 9.5, small: 8.5, name: 24, headline: 11, section: 9, title: 10 },
    lineHeight: 1.3,
    margins: [56, 52, 56, 52],
    header: 'left',
    section: 'rule',
    entry: 'dates-right',
    skills: 'inline',
    gap: { section: 18, entry: 10 },
  },
  {
    id: 'modern',
    name: 'Modern',
    description: 'Clean sans-serif with a blue accent bar on every section.',
    layout: 'single',
    fonts: { body: 'Inter', heading: 'InterSemi', name: 'Inter' },
    colors: { accent: '#0a66c2', text: '#1b1f24', muted: '#5b6470', rule: '#dfe3e8', tag: '#e8f1fb' },
    size: { body: 9.5, small: 8.5, name: 26, headline: 12, section: 11, title: 10.5 },
    lineHeight: 1.32,
    margins: [50, 46, 50, 46],
    header: 'left',
    section: 'bar',
    entry: 'dates-right',
    skills: 'tags',
    gap: { section: 16, entry: 10 },
  },
  {
    id: 'executive',
    name: 'Executive',
    description: 'Serif, centered and formal, for senior and leadership roles.',
    layout: 'single',
    fonts: { body: 'SourceSerif', heading: 'SourceSerif', name: 'SourceSerif' },
    colors: { accent: '#1e3a5f', text: '#1a1a1a', muted: '#555a60', rule: '#b8c2cc', tag: '#eef2f6' },
    size: { body: 10, small: 9, name: 26, headline: 12, section: 10, title: 10.5 },
    lineHeight: 1.3,
    margins: [60, 50, 60, 50],
    header: 'center',
    section: 'caps',
    entry: 'dates-right',
    skills: 'inline',
    gap: { section: 18, entry: 11 },
  },
  {
    id: 'technical',
    name: 'Technical',
    description: 'Monospaced headings and skill tags, built for engineers.',
    layout: 'single',
    fonts: { body: 'Inter', heading: 'CodeMono', name: 'CodeMono', label: 'CodeMono' },
    colors: { accent: '#0f766e', text: '#111827', muted: '#4b5563', rule: '#d1d5db', tag: '#e6f4f2' },
    size: { body: 9.3, small: 8.3, name: 22, headline: 10.5, section: 10, title: 10 },
    lineHeight: 1.3,
    margins: [48, 44, 48, 44],
    header: 'left',
    section: 'mono',
    entry: 'dates-right',
    skills: 'tags',
    gap: { section: 15, entry: 9 },
  },
  {
    id: 'data-ai',
    name: 'Data / AI',
    description: 'A tinted side panel for skills and tools, the story on the right.',
    layout: 'sidebar',
    sidebar: { side: 'left', width: 170, kinds: SIDE_KINDS },
    fonts: { body: 'Inter', heading: 'InterSemi', name: 'InterSemi' },
    colors: {
      accent: '#6d28d9',
      text: '#1e1b2e',
      muted: '#5d5870',
      rule: '#ddd6fe',
      panel: '#f4f1fb',
      panelText: '#231f33',
      panelMuted: '#5d5870',
      tag: '#ede9fe',
    },
    size: { body: 9.3, small: 8.3, name: 22, headline: 11, section: 9.5, title: 10.2 },
    lineHeight: 1.32,
    margins: [36, 42, 42, 42],
    header: 'left',
    section: 'caps',
    entry: 'stacked',
    skills: 'list',
    gap: { section: 15, entry: 10 },
  },
  {
    id: 'consulting',
    name: 'Consulting',
    description: 'Section labels in a left column and a crisp, structured grid.',
    layout: 'single',
    fonts: { body: 'SourceSans', heading: 'SourceSansSemi', name: 'SourceSansSemi' },
    colors: { accent: '#14532d', text: '#16181b', muted: '#50565e', rule: '#cfd6cf', tag: '#e9f2ec' },
    size: { body: 10, small: 9, name: 24, headline: 11.5, section: 9.5, title: 10.5 },
    lineHeight: 1.28,
    margins: [48, 46, 48, 46],
    header: 'split',
    section: 'label',
    entry: 'dates-right',
    skills: 'inline',
    gap: { section: 14, entry: 9 },
  },
  {
    id: 'product',
    name: 'Product',
    description: 'Friendly and confident, with an accent name and underlined sections.',
    layout: 'single',
    fonts: { body: 'Inter', heading: 'InterSemi', name: 'Inter' },
    colors: { accent: '#c2410c', text: '#1c1917', muted: '#57534e', rule: '#e7e5e4', tag: '#fdf0e8' },
    size: { body: 9.5, small: 8.5, name: 26, headline: 12, section: 11.5, title: 10.5 },
    lineHeight: 1.33,
    margins: [52, 46, 52, 46],
    header: 'left',
    section: 'underline',
    entry: 'stacked',
    skills: 'tags',
    gap: { section: 16, entry: 10 },
    accentName: true,
  },
  {
    id: 'creative',
    name: 'Creative',
    description: 'A color band header and elegant display serif headings.',
    layout: 'single',
    fonts: { body: 'SourceSans', heading: 'Playfair', name: 'Playfair' },
    colors: { accent: '#9f1239', text: '#1f1a1c', muted: '#5f5458', rule: '#ecd9de', tag: '#fbeaee' },
    size: { body: 10, small: 9, name: 30, headline: 12.5, section: 14, title: 10.8 },
    lineHeight: 1.3,
    margins: [52, 46, 52, 46],
    header: 'band',
    section: 'serif',
    entry: 'stacked',
    skills: 'tags',
    gap: { section: 16, entry: 10 },
  },
  {
    id: 'academic',
    name: 'Academic',
    description: 'Traditional serif CV with dates in the margin, for research and teaching.',
    layout: 'single',
    fonts: { body: 'SourceSerif', heading: 'SourceSerif', name: 'SourceSerif' },
    colors: { accent: '#1a1a1a', text: '#1a1a1a', muted: '#4a4a4a', rule: '#9a9a9a', tag: '#f1f1f1' },
    size: { body: 10, small: 9, name: 22, headline: 11, section: 10.5, title: 10.3 },
    lineHeight: 1.3,
    margins: [62, 56, 62, 56],
    header: 'center',
    section: 'rule',
    entry: 'dates-left',
    skills: 'inline',
    gap: { section: 18, entry: 10 },
  },
  {
    id: 'compact',
    name: 'Compact',
    description: 'Dense and tidy: fits a long career on one or two pages.',
    layout: 'single',
    fonts: { body: 'SourceSans', heading: 'SourceSansSemi', name: 'SourceSansSemi' },
    colors: { accent: '#0a66c2', text: '#15181c', muted: '#535b64', rule: '#d7dde3', tag: '#e8f1fb' },
    size: { body: 9, small: 8, name: 20, headline: 10.5, section: 9, title: 9.5 },
    lineHeight: 1.22,
    margins: [38, 34, 38, 34],
    header: 'split',
    section: 'caps',
    entry: 'dates-right',
    skills: 'inline',
    gap: { section: 11, entry: 6 },
  },
  {
    id: 'two-column',
    name: 'Two-column',
    description: 'Experience on the left, skills and details in a slim right column.',
    layout: 'columns',
    sidebar: { side: 'right', width: 165, kinds: SIDE_KINDS },
    fonts: { body: 'Inter', heading: 'InterSemi', name: 'InterSemi' },
    colors: { accent: '#0369a1', text: '#16202a', muted: '#566574', rule: '#d6e0e8', tag: '#e6f2f9' },
    size: { body: 9.3, small: 8.3, name: 24, headline: 11.5, section: 9.5, title: 10.2 },
    lineHeight: 1.3,
    margins: [44, 44, 44, 44],
    header: 'left',
    section: 'rule',
    entry: 'stacked',
    skills: 'list',
    gap: { section: 15, entry: 10 },
  },
  {
    id: 'ats',
    name: 'ATS-friendly',
    description: 'Plain single column with standard headings, read reliably by applicant tracking systems.',
    layout: 'single',
    fonts: { body: 'SourceSans', heading: 'SourceSans', name: 'SourceSans' },
    colors: { accent: '#000000', text: '#000000', muted: '#333333', rule: '#000000', tag: '#ffffff' },
    size: { body: 10.5, small: 10, name: 20, headline: 11.5, section: 11, title: 10.5 },
    lineHeight: 1.25,
    margins: [54, 50, 54, 50],
    header: 'left',
    section: 'plain',
    entry: 'stacked',
    skills: 'inline',
    gap: { section: 14, entry: 9 },
    ats: true,
  },
];

export const DEFAULT_TEMPLATE_ID = 'modern';

/** The template with this id, or the default one (e.g. for ids from newer versions). */
export function templateById(id: string): Template {
  return TEMPLATES.find((t) => t.id === id) ?? (TEMPLATES.find((t) => t.id === DEFAULT_TEMPLATE_ID) as Template);
}

/** Accent colors offered in the editor (all readable on white). */
export const ACCENTS: readonly { value: string; label: string }[] = [
  { value: '#0a66c2', label: 'Blue' },
  { value: '#1e3a5f', label: 'Navy' },
  { value: '#0369a1', label: 'Ocean' },
  { value: '#0f766e', label: 'Teal' },
  { value: '#15803d', label: 'Green' },
  { value: '#6d28d9', label: 'Violet' },
  { value: '#9f1239', label: 'Wine' },
  { value: '#c2410c', label: 'Rust' },
  { value: '#b45309', label: 'Amber' },
  { value: '#374151', label: 'Graphite' },
];
