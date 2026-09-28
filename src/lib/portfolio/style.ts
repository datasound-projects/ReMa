/**
 * Design customization on top of a template: curated palettes, font
 * pairings, spacing, layout, dividers, patterns and header treatments.
 * A `PortfolioStyle` holds the user's choices (empty = the template's
 * own); `applyStyle` turns a template plus choices into the effective
 * template the layout engine renders. Reset = an empty style.
 */
import type { PortfolioStyle, SectionKind } from '../../services/portfolioService';
import type { FontName } from './fonts';
import type { DividerStyle, HeaderStyle, PatternStyle, SidebarStyle, Template } from './templates';

export interface Palette {
  id: string;
  name: string;
  accent: string;
  heading: string;
  text: string;
  /** Side panel background. */
  panel: string;
  panelText: string;
}

/** Curated palettes; every text color reads on white and on its panel. */
export const PALETTES: readonly Palette[] = [
  { id: 'ink', name: 'Ink', accent: '#1f2328', heading: '#1f2328', text: '#1f2328', panel: '#f3f4f6', panelText: '#1f2328' },
  { id: 'navy', name: 'Navy', accent: '#1e3a5f', heading: '#1e3a5f', text: '#1a1a1a', panel: '#eef2f6', panelText: '#16202a' },
  { id: 'ocean', name: 'Ocean', accent: '#0369a1', heading: '#0c4a6e', text: '#16202a', panel: '#e6f2f9', panelText: '#16202a' },
  { id: 'teal', name: 'Teal', accent: '#0f766e', heading: '#134e4a', text: '#111827', panel: '#eef6f4', panelText: '#13302c' },
  { id: 'forest', name: 'Forest', accent: '#15803d', heading: '#14532d', text: '#1a2e1f', panel: '#eaf5ec', panelText: '#1a2e1f' },
  { id: 'plum', name: 'Plum', accent: '#6d28d9', heading: '#4c1d95', text: '#1e1b2e', panel: '#f4f1fb', panelText: '#231f33' },
  { id: 'wine', name: 'Wine', accent: '#9f1239', heading: '#881337', text: '#1f1a1c', panel: '#fbeaee', panelText: '#3b1c25' },
  { id: 'terracotta', name: 'Terracotta', accent: '#c2410c', heading: '#7c2d12', text: '#1c1917', panel: '#fdf0e8', panelText: '#3b1f12' },
  { id: 'amber', name: 'Amber', accent: '#b45309', heading: '#78350f', text: '#1c1917', panel: '#fdf3e7', panelText: '#3b2510' },
  { id: 'slate', name: 'Slate', accent: '#475569', heading: '#1e293b', text: '#0f172a', panel: '#f1f5f9', panelText: '#1e293b' },
  { id: 'midnight', name: 'Midnight', accent: '#0f172a', heading: '#0f172a', text: '#0f172a', panel: '#0f172a', panelText: '#f1f5f9' },
  { id: 'charcoal', name: 'Charcoal', accent: '#b45309', heading: '#18181b', text: '#18181b', panel: '#27272a', panelText: '#f4f4f5' },
];

export interface FontPairing {
  id: string;
  name: string;
  body: FontName;
  heading: FontName;
  nameFont: FontName;
  /** For the gallery. */
  sample: string;
}

export const FONT_PAIRINGS: readonly FontPairing[] = [
  { id: 'inter', name: 'Inter', body: 'Inter', heading: 'InterSemi', nameFont: 'InterSemi', sample: 'Clean and neutral' },
  { id: 'source-sans', name: 'Source Sans', body: 'SourceSans', heading: 'SourceSansSemi', nameFont: 'SourceSansSemi', sample: 'Friendly and compact' },
  { id: 'source-serif', name: 'Source Serif', body: 'SourceSerif', heading: 'SourceSerif', nameFont: 'SourceSerif', sample: 'Classic and formal' },
  { id: 'playfair-sans', name: 'Playfair + Source Sans', body: 'SourceSans', heading: 'Playfair', nameFont: 'Playfair', sample: 'Editorial display headings' },
  { id: 'serif-sans', name: 'Source Serif + Inter', body: 'Inter', heading: 'SourceSerif', nameFont: 'SourceSerif', sample: 'Serif headings, sans body' },
  { id: 'inter-serif', name: 'Inter + Source Serif', body: 'SourceSerif', heading: 'InterSemi', nameFont: 'InterSemi', sample: 'Sans headings, serif body' },
  { id: 'mono-inter', name: 'Source Code Pro + Inter', body: 'Inter', heading: 'CodeMono', nameFont: 'InterSemi', sample: 'Monospaced headings' },
];

export const SCALES: readonly { value: number; label: string }[] = [
  { value: 0.9, label: 'Small' },
  { value: 1, label: 'Normal' },
  { value: 1.08, label: 'Large' },
];

export const MARGINS: readonly { value: string; label: string; factor: number }[] = [
  { value: 'narrow', label: 'Narrow', factor: 0.72 },
  { value: 'normal', label: 'Normal', factor: 1 },
  { value: 'wide', label: 'Wide', factor: 1.25 },
];

export const SPACINGS: readonly { value: string; label: string; factor: number }[] = [
  { value: 'tight', label: 'Tight', factor: 0.88 },
  { value: 'normal', label: 'Normal', factor: 1 },
  { value: 'relaxed', label: 'Relaxed', factor: 1.14 },
];

export const COLUMN_LAYOUTS: readonly { value: string; label: string }[] = [
  { value: 'single', label: 'Single column' },
  { value: 'sidebar_left', label: 'Side panel left' },
  { value: 'sidebar_right', label: 'Side panel right' },
  { value: 'columns_right', label: 'Two columns' },
];

export const PATTERNS: readonly { value: PatternStyle; label: string }[] = [
  { value: 'none', label: 'None' },
  { value: 'dots', label: 'Dots' },
  { value: 'grid', label: 'Grid' },
  { value: 'diagonal', label: 'Diagonal' },
];

export const DIVIDERS: readonly { value: DividerStyle; label: string }[] = [
  { value: 'none', label: 'None' },
  { value: 'hairline', label: 'Hairline' },
  { value: 'dotted', label: 'Dotted' },
  { value: 'thick', label: 'Thick' },
];

export const HEADERS: readonly { value: HeaderStyle; label: string }[] = [
  { value: 'left', label: 'Left' },
  { value: 'center', label: 'Centered' },
  { value: 'split', label: 'Split' },
  { value: 'stacked', label: 'Stacked' },
  { value: 'band', label: 'Color band' },
];

export const SIDEBARS: readonly { value: SidebarStyle; label: string }[] = [
  { value: 'tinted', label: 'Tinted' },
  { value: 'plain', label: 'Plain' },
  { value: 'outlined', label: 'Outlined' },
];

export function defaultStyle(): Required<PortfolioStyle> {
  return {
    palette: '',
    headingColor: '',
    textColor: '',
    backgroundColor: '',
    panelColor: '',
    fontPairing: '',
    scale: 0,
    margins: '',
    lineSpacing: '',
    sectionSpacing: '',
    columns: '',
    pattern: '',
    dividers: '',
    header: '',
    sidebar: '',
    textAlign: '',
    showPhoto: false,
  };
}

/** Fills in every field (stored styles may be partial or missing). */
export function normalizeStyle(style: PortfolioStyle | null | undefined): Required<PortfolioStyle> {
  const base = defaultStyle();
  if (!style) return base;
  return {
    ...base,
    ...Object.fromEntries(Object.entries(style).filter(([, v]) => v !== undefined && v !== null)),
  } as Required<PortfolioStyle>;
}

export function isDefaultStyle(style: PortfolioStyle | null | undefined): boolean {
  const s = normalizeStyle(style);
  return Object.entries(s).every(([, v]) => v === '' || v === 0 || v === false);
}

const HEX = /^#[0-9a-f]{6}$/i;
const hex = (value: string | undefined): string | null => (value && HEX.test(value.trim()) ? value.trim().toLowerCase() : null);

/** Relative luminance (WCAG) of a `#rrggbb` color. */
export function luminance(color: string): number {
  const match = /^#?([0-9a-f]{6})$/i.exec(color.trim());
  if (!match) return 1;
  const n = parseInt(match[1] ?? 'ffffff', 16);
  const channel = (c: number) => {
    const s = c / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * channel((n >> 16) & 0xff) + 0.7152 * channel((n >> 8) & 0xff) + 0.0722 * channel(n & 0xff);
}

export function contrastRatio(a: string, b: string): number {
  const la = luminance(a);
  const lb = luminance(b);
  return (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05);
}

/** Black or white, whichever reads better on `background`. */
export function readableOn(background: string): string {
  return contrastRatio(background, '#1a1a1a') >= contrastRatio(background, '#ffffff') ? '#1a1a1a' : '#ffffff';
}

/** Keeps a chosen text color readable on its background, or falls back. */
function readable(color: string | null, background: string, fallback: string): string {
  if (!color) return fallback;
  return contrastRatio(color, background) >= 3 ? color : fallback;
}

/** The effective template: the base design with the user's choices applied. */
export function applyStyle(base: Template, style: PortfolioStyle | null | undefined, accent: string): Template {
  const s = normalizeStyle(style);
  const t: Template = { ...base, fonts: { ...base.fonts }, colors: { ...base.colors }, size: { ...base.size }, gap: { ...base.gap }, margins: [...base.margins] };
  if (t.plain) {
    // Text-first templates keep their black-and-white design; only spacing changes.
    applySpacing(t, s);
    return t;
  }

  const palette = PALETTES.find((p) => p.id === s.palette);
  const background = hex(s.backgroundColor);
  if (background && luminance(background) >= 0.5) t.colors.background = background;
  const page = t.colors.background ?? '#ffffff';

  const chosenAccent = hex(accent) ?? palette?.accent ?? t.colors.accent;
  t.colors.accent = chosenAccent;
  if (palette) {
    t.colors.heading = palette.heading;
    t.colors.text = palette.text;
    t.colors.panel = palette.panel;
    t.colors.panelText = palette.panelText;
    t.colors.panelMuted = palette.panelText === '#f1f5f9' || palette.panelText === '#f4f4f5' ? '#cbd5e1' : palette.panelText;
    t.colors.tag = undefined;
  }
  t.colors.text = readable(hex(s.textColor), page, t.colors.text);
  const heading = readable(hex(s.headingColor), page, t.colors.heading ?? t.colors.text);
  if (heading !== (t.colors.heading ?? t.colors.text)) t.colors.heading = heading;
  const panel = hex(s.panelColor);
  if (panel) {
    t.colors.panel = panel;
    t.colors.panelText = readableOn(panel);
    t.colors.panelMuted = t.colors.panelText === '#ffffff' ? '#d4dae3' : '#4b5563';
  }

  const pairing = FONT_PAIRINGS.find((p) => p.id === s.fontPairing);
  if (pairing) {
    t.fonts = { body: pairing.body, heading: pairing.heading, name: pairing.nameFont, ...(pairing.heading === 'CodeMono' ? { label: 'CodeMono' as const } : {}) };
  }

  const layout = s.columns;
  if (layout === 'single') {
    t.layout = 'single';
    t.sidebar = undefined;
  } else if (layout === 'sidebar_left' || layout === 'sidebar_right') {
    t.layout = 'sidebar';
    t.sidebar = { side: layout === 'sidebar_left' ? 'left' : 'right', width: t.sidebar?.width ?? 165, kinds: [...(t.sidebar?.kinds ?? SIDE_KINDS_DEFAULT)], style: t.sidebar?.style ?? 'tinted' };
  } else if (layout === 'columns_right') {
    t.layout = 'columns';
    t.sidebar = { side: 'right', width: t.sidebar?.width ?? 160, kinds: [...(t.sidebar?.kinds ?? SIDE_KINDS_DEFAULT)], style: 'plain' };
  }
  if (t.sidebar && s.sidebar) t.sidebar = { ...t.sidebar, style: s.sidebar as SidebarStyle };
  if (s.pattern) t.pattern = s.pattern as PatternStyle;
  if (s.dividers) t.dividers = s.dividers as DividerStyle;
  if (s.header) t.header = s.header as HeaderStyle;
  if (s.textAlign) t.justify = s.textAlign === 'justify';
  applySpacing(t, s);
  return t;
}

const SIDE_KINDS_DEFAULT: SectionKind[] = ['skills', 'languages', 'links', 'certifications'];

function applySpacing(t: Template, s: Required<PortfolioStyle>) {
  const scale = s.scale && s.scale > 0 ? s.scale : 1;
  if (scale !== 1) {
    for (const key of Object.keys(t.size) as (keyof Template['size'])[]) {
      t.size[key] = Math.round(t.size[key] * scale * 10) / 10;
    }
  }
  const margin = MARGINS.find((m) => m.value === s.margins);
  if (margin && margin.factor !== 1) {
    t.margins = t.margins.map((m) => Math.round(m * margin.factor)) as [number, number, number, number];
  }
  const line = SPACINGS.find((m) => m.value === s.lineSpacing);
  if (line && line.factor !== 1) t.lineHeight = Math.round(t.lineHeight * line.factor * 100) / 100;
  const section = SPACINGS.find((m) => m.value === s.sectionSpacing);
  if (section && section.factor !== 1) {
    t.gap = { section: Math.round(t.gap.section * section.factor), entry: Math.round(t.gap.entry * section.factor) };
  }
}
