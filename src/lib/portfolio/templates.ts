/**
 * Portfolio Studio templates: a registry of designs. A template is data —
 * layout, type, color and spacing choices — read by one layout engine
 * (`layout.ts`), so adding a design means adding an entry here. Content
 * never depends on the template: switching keeps everything.
 *
 * CV templates come in ten categories (two each); any template works for
 * any profession. Cover letter templates share the header treatments and
 * fonts so a CV and a letter can match.
 */
import type { SectionKind } from '../../services/portfolioService';
import type { FontName } from './fonts';

export type HeaderStyle = 'left' | 'center' | 'band' | 'split' | 'stacked' | 'monogram';
/** How section titles look. */
export type SectionStyle =
  | 'rule'
  | 'caps'
  | 'bar'
  | 'underline'
  | 'mono'
  | 'serif'
  | 'label'
  | 'plain'
  | 'numbered'
  | 'boxed'
  | 'side-rule'
  | 'smallcaps'
  | 'thin-caps';
/** Where dates go in an entry. */
export type EntryStyle = 'stacked' | 'dates-right' | 'dates-left' | 'table' | 'timeline';
export type SkillStyle = 'inline' | 'tags' | 'list';
export type DividerStyle = 'none' | 'hairline' | 'dotted' | 'thick';
/** Subtle patterns, drawn only behind decorative areas (bands, side panels). */
export type PatternStyle = 'none' | 'dots' | 'grid' | 'diagonal';
export type SidebarStyle = 'tinted' | 'plain' | 'outlined';
export type PhotoStyle = 'circle' | 'square';
export type TemplateKind = 'cv' | 'letter';

export type Category =
  | 'Technology & Engineering'
  | 'Design & Creative'
  | 'Business & Operations'
  | 'Finance & Accounting'
  | 'Marketing & Communications'
  | 'Sales & Customer Success'
  | 'Healthcare & Care'
  | 'Education & Research'
  | 'Legal & Administration'
  | 'Management & Leadership'
  | 'Cover letters';

export const CATEGORIES: readonly Category[] = [
  'Technology & Engineering',
  'Design & Creative',
  'Business & Operations',
  'Finance & Accounting',
  'Marketing & Communications',
  'Sales & Customer Success',
  'Healthcare & Care',
  'Education & Research',
  'Legal & Administration',
  'Management & Leadership',
];

/** Style tags shown as filters. */
export type StyleTag =
  | 'single-column'
  | 'two-column'
  | 'sidebar'
  | 'minimal'
  | 'editorial'
  | 'executive'
  | 'modern'
  | 'classic'
  | 'compact'
  | 'text-first'
  | 'photo';

export interface Template {
  id: string;
  kind: TemplateKind;
  name: string;
  category: Category;
  tags: StyleTag[];
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
    style?: SidebarStyle;
  };
  fonts: { body: FontName; heading: FontName; name: FontName; label?: FontName };
  colors: {
    /** Default accent; the user can pick another. */
    accent: string;
    text: string;
    muted: string;
    rule: string;
    /** Headings, when not the text color. */
    heading?: string;
    /** Page background (white unless set). */
    background?: string;
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
  dividers?: DividerStyle;
  pattern?: PatternStyle;
  /** Where a photo goes when the document has one and shows it. */
  photo?: PhotoStyle;
  /** Name in the accent color. */
  accentName?: boolean;
  /** Justified body text. */
  justify?: boolean;
  /** Text only: no graphics, columns, chips or colored rules. */
  plain?: boolean;
}

const SIDE_KINDS: SectionKind[] = ['skills', 'languages', 'links', 'certifications'];
const SIDE_KINDS_WIDE: SectionKind[] = ['skills', 'languages', 'links', 'certifications', 'education'];

const cv = (t: Omit<Template, 'kind'>): Template => ({ kind: 'cv', ...t });
const letter = (t: Omit<Template, 'kind' | 'category' | 'entry' | 'skills' | 'section'> & Partial<Pick<Template, 'section'>>): Template => ({
  kind: 'letter',
  category: 'Cover letters',
  entry: 'stacked',
  skills: 'inline',
  section: 'plain',
  ...t,
});

export const TEMPLATES: readonly Template[] = [
  // ── Technology & Engineering ──────────────────────────────────────
  cv({
    id: 'technical-grid',
    name: 'Technical Grid',
    category: 'Technology & Engineering',
    tags: ['sidebar', 'modern'],
    description: 'A tinted side panel for skills and tools, monospaced section titles, the story on the right.',
    layout: 'sidebar',
    sidebar: { side: 'left', width: 168, kinds: SIDE_KINDS, style: 'tinted' },
    fonts: { body: 'Inter', heading: 'CodeMono', name: 'InterSemi', label: 'CodeMono' },
    colors: {
      accent: '#0f766e',
      text: '#111827',
      muted: '#4b5563',
      rule: '#cfd8d6',
      panel: '#eef6f4',
      panelText: '#13302c',
      panelMuted: '#4b5f5b',
      tag: '#e6f4f2',
    },
    size: { body: 9.3, small: 8.3, name: 22, headline: 10.5, section: 9.5, title: 10 },
    lineHeight: 1.3,
    margins: [36, 42, 42, 42],
    header: 'left',
    section: 'mono',
    entry: 'dates-right',
    skills: 'list',
    gap: { section: 15, entry: 9 },
    pattern: 'grid',
  }),
  cv({
    id: 'systems-minimal',
    name: 'Systems Minimal',
    category: 'Technology & Engineering',
    tags: ['single-column', 'minimal'],
    description: 'Quiet and precise: black type, fine rules, lots of air, dates on the right.',
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
  }),

  // ── Design & Creative ─────────────────────────────────────────────
  cv({
    id: 'editorial-portfolio',
    name: 'Editorial Portfolio',
    category: 'Design & Creative',
    tags: ['two-column', 'editorial', 'photo'],
    description: 'A large display name, italic serif section titles and a slim right column, magazine style.',
    layout: 'columns',
    sidebar: { side: 'right', width: 150, kinds: SIDE_KINDS, style: 'plain' },
    fonts: { body: 'SourceSans', heading: 'Playfair', name: 'Playfair' },
    colors: { accent: '#1c1917', text: '#1c1917', muted: '#68625d', rule: '#d6d1cc', heading: '#1c1917', tag: '#f1ede9' },
    size: { body: 9.6, small: 8.6, name: 34, headline: 12, section: 13, title: 10.5 },
    lineHeight: 1.32,
    margins: [50, 48, 50, 48],
    header: 'stacked',
    section: 'serif',
    entry: 'stacked',
    skills: 'list',
    gap: { section: 18, entry: 10 },
    dividers: 'hairline',
    photo: 'circle',
  }),
  cv({
    id: 'studio-accent',
    name: 'Studio Accent',
    category: 'Design & Creative',
    tags: ['single-column', 'modern'],
    description: 'A color band header with a subtle dot pattern, an accent name and underlined sections.',
    layout: 'single',
    fonts: { body: 'SourceSans', heading: 'InterSemi', name: 'Inter' },
    colors: { accent: '#9f1239', text: '#1f1a1c', muted: '#5f5458', rule: '#ecd9de', tag: '#fbeaee' },
    size: { body: 10, small: 9, name: 28, headline: 12, section: 11.5, title: 10.6 },
    lineHeight: 1.3,
    margins: [52, 46, 52, 46],
    header: 'band',
    section: 'underline',
    entry: 'stacked',
    skills: 'tags',
    gap: { section: 16, entry: 10 },
    pattern: 'dots',
    photo: 'square',
  }),

  // ── Business & Operations ─────────────────────────────────────────
  cv({
    id: 'structured-professional',
    name: 'Structured Professional',
    category: 'Business & Operations',
    tags: ['single-column', 'classic'],
    description: 'Section labels in a left column and a crisp, structured grid of entries.',
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
  }),
  cv({
    id: 'operations-clear',
    name: 'Operations Clear',
    category: 'Business & Operations',
    tags: ['single-column', 'compact'],
    description: 'Dense and tidy with dates in a narrow column: fits a long career on one or two pages.',
    layout: 'single',
    fonts: { body: 'SourceSans', heading: 'SourceSansSemi', name: 'SourceSansSemi' },
    colors: { accent: '#0a66c2', text: '#15181c', muted: '#535b64', rule: '#d7dde3', tag: '#e8f1fb' },
    size: { body: 9, small: 8, name: 20, headline: 10.5, section: 9, title: 9.5 },
    lineHeight: 1.22,
    margins: [38, 34, 38, 34],
    header: 'split',
    section: 'caps',
    entry: 'table',
    skills: 'inline',
    gap: { section: 11, entry: 6 },
    dividers: 'hairline',
  }),

  // ── Finance & Accounting ──────────────────────────────────────────
  cv({
    id: 'precision-classic',
    name: 'Precision Classic',
    category: 'Finance & Accounting',
    tags: ['single-column', 'classic'],
    description: 'Centered serif header, ruled sections and dates in the margin: conservative and exact.',
    layout: 'single',
    fonts: { body: 'SourceSerif', heading: 'SourceSerif', name: 'SourceSerif' },
    colors: { accent: '#1e3a5f', text: '#1a1a1a', muted: '#555a60', rule: '#b8c2cc', tag: '#eef2f6' },
    size: { body: 10, small: 9, name: 24, headline: 11.5, section: 10, title: 10.5 },
    lineHeight: 1.3,
    margins: [60, 50, 60, 50],
    header: 'center',
    section: 'rule',
    entry: 'dates-left',
    skills: 'inline',
    gap: { section: 17, entry: 10 },
  }),
  cv({
    id: 'finance-modern',
    name: 'Finance Modern',
    category: 'Finance & Accounting',
    tags: ['single-column', 'modern'],
    description: 'A sans-serif stacked header with a thin accent line beside every section title.',
    layout: 'single',
    fonts: { body: 'Inter', heading: 'InterSemi', name: 'InterSemi' },
    colors: { accent: '#0369a1', text: '#16202a', muted: '#566574', rule: '#d6e0e8', tag: '#e6f2f9' },
    size: { body: 9.5, small: 8.5, name: 24, headline: 11.5, section: 10.5, title: 10.3 },
    lineHeight: 1.32,
    margins: [50, 46, 50, 46],
    header: 'stacked',
    section: 'side-rule',
    entry: 'dates-right',
    skills: 'inline',
    gap: { section: 16, entry: 10 },
  }),

  // ── Marketing & Communications ────────────────────────────────────
  cv({
    id: 'campaign-modern',
    name: 'Campaign Modern',
    category: 'Marketing & Communications',
    tags: ['single-column', 'modern'],
    description: 'Clean sans-serif with a blue accent bar on every section and skill chips.',
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
  }),
  cv({
    id: 'brand-story',
    name: 'Brand Story',
    category: 'Marketing & Communications',
    tags: ['two-column', 'editorial'],
    description: 'A diagonal-patterned color band, display serif headings and a right column for skills.',
    layout: 'columns',
    sidebar: { side: 'right', width: 160, kinds: SIDE_KINDS, style: 'outlined' },
    fonts: { body: 'SourceSans', heading: 'Playfair', name: 'Playfair' },
    colors: { accent: '#c2410c', text: '#1c1917', muted: '#57534e', rule: '#e7e5e4', tag: '#fdf0e8' },
    size: { body: 9.6, small: 8.6, name: 28, headline: 12, section: 13, title: 10.6 },
    lineHeight: 1.3,
    margins: [48, 46, 48, 46],
    header: 'band',
    section: 'serif',
    entry: 'stacked',
    skills: 'list',
    gap: { section: 16, entry: 10 },
    pattern: 'diagonal',
  }),

  // ── Sales & Customer Success ──────────────────────────────────────
  cv({
    id: 'results-focus',
    name: 'Results Focus',
    category: 'Sales & Customer Success',
    tags: ['single-column', 'modern'],
    description: 'Bold filled section titles and an accent name: built around numbers and outcomes.',
    layout: 'single',
    fonts: { body: 'Inter', heading: 'InterSemi', name: 'InterSemi' },
    colors: { accent: '#b45309', text: '#1c1917', muted: '#57534e', rule: '#e7e5e4', tag: '#fdf3e7' },
    size: { body: 9.5, small: 8.5, name: 26, headline: 11.5, section: 9.5, title: 10.5 },
    lineHeight: 1.3,
    margins: [48, 44, 48, 44],
    header: 'left',
    section: 'boxed',
    entry: 'dates-right',
    skills: 'tags',
    gap: { section: 16, entry: 10 },
    accentName: true,
  }),
  cv({
    id: 'relationship-professional',
    name: 'Relationship Professional',
    category: 'Sales & Customer Success',
    tags: ['two-column', 'classic'],
    description: 'Experience on the left, skills and details in a slim right column, warm and approachable.',
    layout: 'columns',
    sidebar: { side: 'right', width: 165, kinds: SIDE_KINDS, style: 'plain' },
    fonts: { body: 'SourceSans', heading: 'SourceSansSemi', name: 'SourceSansSemi' },
    colors: { accent: '#15803d', text: '#16202a', muted: '#566574', rule: '#d6e0d8', tag: '#e8f5ec' },
    size: { body: 9.5, small: 8.5, name: 24, headline: 11.5, section: 9.5, title: 10.3 },
    lineHeight: 1.3,
    margins: [44, 44, 44, 44],
    header: 'left',
    section: 'rule',
    entry: 'stacked',
    skills: 'list',
    gap: { section: 15, entry: 10 },
  }),

  // ── Healthcare & Care ─────────────────────────────────────────────
  cv({
    id: 'clinical-clear',
    name: 'Clinical Clear',
    category: 'Healthcare & Care',
    tags: ['single-column', 'text-first'],
    description: 'Plain bold headings with hairline dividers: restrained, legible and text-first.',
    layout: 'single',
    fonts: { body: 'SourceSans', heading: 'SourceSansSemi', name: 'SourceSansSemi' },
    colors: { accent: '#0e7490', text: '#111827', muted: '#4b5563', rule: '#cbd5e1', tag: '#e6f4f8' },
    size: { body: 10, small: 9, name: 22, headline: 11.5, section: 10.5, title: 10.5 },
    lineHeight: 1.3,
    margins: [54, 48, 54, 48],
    header: 'left',
    section: 'plain',
    entry: 'dates-right',
    skills: 'inline',
    gap: { section: 15, entry: 9 },
    dividers: 'hairline',
  }),
  cv({
    id: 'care-professional',
    name: 'Care Professional',
    category: 'Healthcare & Care',
    tags: ['sidebar', 'photo'],
    description: 'A soft green side panel with a photo, contact details and skills; a warm, calm layout.',
    layout: 'sidebar',
    sidebar: { side: 'left', width: 160, kinds: SIDE_KINDS_WIDE, style: 'tinted' },
    fonts: { body: 'SourceSans', heading: 'SourceSansSemi', name: 'SourceSansSemi' },
    colors: {
      accent: '#15803d',
      text: '#1a2e1f',
      muted: '#4f6355',
      rule: '#cfe3d4',
      panel: '#eaf5ec',
      panelText: '#1a2e1f',
      panelMuted: '#4f6355',
      tag: '#e8f5ec',
    },
    size: { body: 9.5, small: 8.5, name: 22, headline: 11, section: 9.5, title: 10.3 },
    lineHeight: 1.32,
    margins: [36, 42, 42, 42],
    header: 'left',
    section: 'caps',
    entry: 'stacked',
    skills: 'list',
    gap: { section: 15, entry: 10 },
    photo: 'circle',
  }),

  // ── Education & Research ──────────────────────────────────────────
  cv({
    id: 'academic-profile',
    name: 'Academic Profile',
    category: 'Education & Research',
    tags: ['single-column', 'classic'],
    description: 'Traditional serif CV with thin capital headings and dates in the margin, for research, teaching and publications.',
    layout: 'single',
    fonts: { body: 'SourceSerif', heading: 'SourceSerif', name: 'SourceSerif' },
    colors: { accent: '#1a1a1a', text: '#1a1a1a', muted: '#4a4a4a', rule: '#9a9a9a', tag: '#f1f1f1' },
    size: { body: 10, small: 9, name: 22, headline: 11, section: 9.5, title: 10.3 },
    lineHeight: 1.3,
    margins: [62, 56, 62, 56],
    header: 'center',
    section: 'thin-caps',
    entry: 'dates-left',
    skills: 'inline',
    gap: { section: 18, entry: 10 },
  }),
  cv({
    id: 'teaching-modern',
    name: 'Teaching Modern',
    category: 'Education & Research',
    tags: ['single-column', 'modern'],
    description: 'A friendly split header, numbered sections and stacked entries in a warm amber.',
    layout: 'single',
    fonts: { body: 'SourceSans', heading: 'InterSemi', name: 'InterSemi' },
    colors: { accent: '#b45309', text: '#1c1917', muted: '#57534e', rule: '#e7e5e4', tag: '#fdf3e7' },
    size: { body: 9.8, small: 8.8, name: 24, headline: 11.5, section: 10.5, title: 10.5 },
    lineHeight: 1.32,
    margins: [52, 46, 52, 46],
    header: 'split',
    section: 'numbered',
    entry: 'stacked',
    skills: 'tags',
    gap: { section: 16, entry: 10 },
  }),

  // ── Legal & Administration ────────────────────────────────────────
  cv({
    id: 'formal-counsel',
    name: 'Formal Counsel',
    category: 'Legal & Administration',
    tags: ['single-column', 'classic', 'executive'],
    description: 'Serif, centered, with small-caps section titles between rules: formal and exact.',
    layout: 'single',
    fonts: { body: 'SourceSerif', heading: 'SourceSerif', name: 'SourceSerif' },
    colors: { accent: '#3f3f46', text: '#18181b', muted: '#52525b', rule: '#a1a1aa', tag: '#f4f4f5' },
    size: { body: 10, small: 9, name: 24, headline: 11, section: 9.5, title: 10.5 },
    lineHeight: 1.32,
    margins: [64, 54, 64, 54],
    header: 'center',
    section: 'smallcaps',
    entry: 'dates-right',
    skills: 'inline',
    gap: { section: 18, entry: 10 },
    justify: true,
  }),
  cv({
    id: 'administrative-essential',
    name: 'Administrative Essential',
    category: 'Legal & Administration',
    tags: ['single-column', 'text-first', 'minimal'],
    description: 'Text only, single column, standard headings and no graphics: the most restrained layout.',
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
    plain: true,
  }),

  // ── Management & Leadership ───────────────────────────────────────
  cv({
    id: 'executive-brief',
    name: 'Executive Brief',
    category: 'Management & Leadership',
    tags: ['single-column', 'executive'],
    description: 'Serif, a stacked header over a thick rule and thin capital section titles, for senior roles.',
    layout: 'single',
    fonts: { body: 'SourceSerif', heading: 'SourceSerif', name: 'SourceSerif' },
    colors: { accent: '#1e3a5f', text: '#1a1a1a', muted: '#555a60', rule: '#b8c2cc', tag: '#eef2f6' },
    size: { body: 10, small: 9, name: 26, headline: 12, section: 9.5, title: 10.6 },
    lineHeight: 1.32,
    margins: [60, 50, 60, 50],
    header: 'stacked',
    section: 'thin-caps',
    entry: 'dates-right',
    skills: 'inline',
    gap: { section: 18, entry: 11 },
    dividers: 'thick',
  }),
  cv({
    id: 'leadership-profile',
    name: 'Leadership Profile',
    category: 'Management & Leadership',
    tags: ['sidebar', 'executive', 'modern', 'photo'],
    description: 'A monogram header and a dark right-hand panel for skills, languages and contact details.',
    layout: 'sidebar',
    sidebar: { side: 'right', width: 165, kinds: SIDE_KINDS, style: 'tinted' },
    fonts: { body: 'Inter', heading: 'InterSemi', name: 'InterSemi' },
    colors: {
      accent: '#1e3a5f',
      text: '#16202a',
      muted: '#566574',
      rule: '#d6dde5',
      panel: '#1e3a5f',
      panelText: '#f3f6fa',
      panelMuted: '#b7c4d4',
      tag: '#e6ecf3',
    },
    size: { body: 9.5, small: 8.5, name: 23, headline: 11, section: 9.5, title: 10.3 },
    lineHeight: 1.32,
    margins: [44, 44, 36, 44],
    header: 'monogram',
    section: 'caps',
    entry: 'dates-right',
    skills: 'list',
    gap: { section: 16, entry: 10 },
    photo: 'circle',
  }),

  // ── Cover letters ─────────────────────────────────────────────────
  letter({
    id: 'letter-classic',
    name: 'Classic Professional',
    tags: ['classic', 'single-column'],
    description: 'A traditional letter: name and contact details on top, a rule, the date and recipient block.',
    layout: 'single',
    fonts: { body: 'SourceSerif', heading: 'SourceSerif', name: 'SourceSerif' },
    colors: { accent: '#1e3a5f', text: '#1a1a1a', muted: '#555a60', rule: '#b8c2cc' },
    size: { body: 10.5, small: 9, name: 22, headline: 11, section: 10, title: 10.5 },
    lineHeight: 1.4,
    margins: [64, 56, 64, 56],
    header: 'center',
    gap: { section: 14, entry: 8 },
  }),
  letter({
    id: 'letter-minimal',
    name: 'Modern Minimal',
    tags: ['minimal', 'modern', 'single-column'],
    description: 'Sans-serif, left-aligned, a thin rule and generous margins: nothing but the words.',
    layout: 'single',
    fonts: { body: 'Inter', heading: 'InterSemi', name: 'InterSemi' },
    colors: { accent: '#374151', text: '#1f2328', muted: '#6b7280', rule: '#d1d5db' },
    size: { body: 10, small: 8.5, name: 20, headline: 10.5, section: 9, title: 10 },
    lineHeight: 1.45,
    margins: [64, 60, 64, 60],
    header: 'left',
    gap: { section: 14, entry: 8 },
  }),
  letter({
    id: 'letter-executive',
    name: 'Executive',
    tags: ['executive', 'classic', 'single-column'],
    description: 'A stacked serif header over a thick rule, matching the Executive Brief CV.',
    layout: 'single',
    fonts: { body: 'SourceSerif', heading: 'SourceSerif', name: 'SourceSerif' },
    colors: { accent: '#1e3a5f', text: '#1a1a1a', muted: '#555a60', rule: '#b8c2cc' },
    size: { body: 10.5, small: 9, name: 24, headline: 11.5, section: 10, title: 10.5 },
    lineHeight: 1.4,
    margins: [60, 50, 60, 56],
    header: 'stacked',
    gap: { section: 14, entry: 8 },
    dividers: 'thick',
  }),
  letter({
    id: 'letter-editorial',
    name: 'Creative Editorial',
    tags: ['editorial', 'single-column'],
    description: 'A color band with the sender’s name in a display serif, matching the Studio and Brand CVs.',
    layout: 'single',
    fonts: { body: 'SourceSans', heading: 'Playfair', name: 'Playfair' },
    colors: { accent: '#9f1239', text: '#1f1a1c', muted: '#5f5458', rule: '#ecd9de' },
    size: { body: 10.2, small: 9, name: 26, headline: 11.5, section: 11, title: 10.5 },
    lineHeight: 1.42,
    margins: [56, 46, 56, 52],
    header: 'band',
    gap: { section: 14, entry: 8 },
    pattern: 'dots',
  }),
  letter({
    id: 'letter-graduate',
    name: 'Graduate / Career Change',
    tags: ['modern', 'single-column'],
    description: 'A split header with contact details on the right and a friendly accent, room for a longer story.',
    layout: 'single',
    fonts: { body: 'SourceSans', heading: 'SourceSansSemi', name: 'SourceSansSemi' },
    colors: { accent: '#0f766e', text: '#16202a', muted: '#566574', rule: '#cfe0dd' },
    size: { body: 10.2, small: 9, name: 21, headline: 11, section: 10, title: 10.3 },
    lineHeight: 1.42,
    margins: [58, 50, 58, 54],
    header: 'split',
    gap: { section: 13, entry: 8 },
  }),
];

export const DEFAULT_TEMPLATE_ID = 'campaign-modern';
export const DEFAULT_LETTER_TEMPLATE_ID = 'letter-classic';

/** Ids from earlier versions of ReMa, kept so stored documents open in the closest design. */
const ALIASES: Record<string, string> = {
  minimal: 'systems-minimal',
  modern: 'campaign-modern',
  executive: 'executive-brief',
  technical: 'technical-grid',
  'data-ai': 'technical-grid',
  consulting: 'structured-professional',
  product: 'results-focus',
  creative: 'studio-accent',
  academic: 'academic-profile',
  compact: 'operations-clear',
  'two-column': 'relationship-professional',
  ats: 'administrative-essential',
};

export const CV_TEMPLATES: readonly Template[] = TEMPLATES.filter((t) => t.kind === 'cv');
export const LETTER_TEMPLATES: readonly Template[] = TEMPLATES.filter((t) => t.kind === 'letter');

/** The template with this id (or an old alias), or the default one of its kind. */
export function templateById(id: string, kind: TemplateKind = 'cv'): Template {
  const resolved = ALIASES[id] ?? id;
  const found = TEMPLATES.find((t) => t.id === resolved);
  if (found) return found;
  const fallback = kind === 'letter' ? DEFAULT_LETTER_TEMPLATE_ID : DEFAULT_TEMPLATE_ID;
  return TEMPLATES.find((t) => t.id === fallback) as Template;
}

/** The letter template that matches a CV template's fonts and header, for a matching pair. */
export function matchingLetterTemplate(cvTemplateId: string): Template {
  const t = templateById(cvTemplateId);
  const serif = t.fonts.body === 'SourceSerif';
  if (t.header === 'band' || t.fonts.heading === 'Playfair') return templateById('letter-editorial', 'letter');
  if (t.header === 'stacked' && serif) return templateById('letter-executive', 'letter');
  if (serif) return templateById('letter-classic', 'letter');
  if (t.header === 'split') return templateById('letter-graduate', 'letter');
  return templateById('letter-minimal', 'letter');
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
