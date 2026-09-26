/**
 * Editing helpers for Portfolio Studio documents (pure functions).
 */
import type {
  PortfolioContent,
  PortfolioEntry,
  PortfolioSection,
  SectionKind,
} from '../../services/portfolioService';

export const SECTION_KINDS: { kind: SectionKind; label: string; description: string }[] = [
  { kind: 'summary', label: 'Summary', description: 'A short introduction' },
  { kind: 'experience', label: 'Experience', description: 'Roles and achievements' },
  { kind: 'projects', label: 'Projects', description: 'Things you built or led' },
  { kind: 'education', label: 'Education', description: 'Degrees and schools' },
  { kind: 'skills', label: 'Skills', description: 'Groups of skills and tools' },
  { kind: 'languages', label: 'Languages', description: 'Languages and levels' },
  { kind: 'certifications', label: 'Certifications', description: 'Certificates and licenses' },
  { kind: 'links', label: 'Links', description: 'Portfolio, publications, profiles' },
  { kind: 'custom', label: 'Custom section', description: 'Anything else: awards, volunteering…' },
];

export const DEFAULT_TITLES: Record<SectionKind, string> = {
  summary: 'Summary',
  experience: 'Experience',
  projects: 'Projects',
  education: 'Education',
  skills: 'Skills',
  languages: 'Languages',
  certifications: 'Certifications',
  links: 'Links',
  custom: 'Additional information',
};

/** Which fields an entry of a section kind uses (the editor shows only these). */
export interface EntryFields {
  title: string;
  subtitle?: string;
  location?: boolean;
  dates?: 'range' | 'single';
  url?: boolean;
  description?: boolean;
  tags?: string;
}

export const ENTRY_FIELDS: Record<SectionKind, EntryFields | null> = {
  summary: null,
  experience: { title: 'Role', subtitle: 'Company', location: true, dates: 'range', description: true },
  projects: { title: 'Project', subtitle: 'Role or context', dates: 'range', url: true, description: true, tags: 'Tools' },
  education: { title: 'Degree', subtitle: 'School', location: true, dates: 'range', description: true },
  skills: { title: 'Group (optional)', tags: 'Skills' },
  languages: { title: 'Language', subtitle: 'Level' },
  certifications: { title: 'Certificate', subtitle: 'Issuer', dates: 'single', url: true },
  links: { title: 'Label', url: true },
  custom: { title: 'Title', subtitle: 'Subtitle', dates: 'range', url: true, description: true },
};

/** Sections with free text (instead of, or as well as, entries). */
export const HAS_TEXT: Record<SectionKind, boolean> = {
  summary: true,
  experience: false,
  projects: false,
  education: false,
  skills: false,
  languages: false,
  certifications: false,
  links: false,
  custom: true,
};

export function newId(): string {
  const bytes = new Uint8Array(6);
  crypto.getRandomValues(bytes);
  return Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('');
}

export function newEntry(): PortfolioEntry {
  return { id: newId(), title: '', subtitle: '', location: '', start: '', end: '', url: '', description: '', tags: [] };
}

export function newSection(kind: SectionKind): PortfolioSection {
  return {
    id: newId(),
    kind,
    title: DEFAULT_TITLES[kind],
    visible: true,
    text: '',
    entries: ENTRY_FIELDS[kind] && kind !== 'custom' ? [newEntry()] : [],
  };
}

export function updateSection(
  content: PortfolioContent,
  id: string,
  update: (section: PortfolioSection) => PortfolioSection,
): PortfolioContent {
  return { ...content, sections: content.sections.map((s) => (s.id === id ? update(s) : s)) };
}

export function moveSection(content: PortfolioContent, id: string, delta: -1 | 1): PortfolioContent {
  const index = content.sections.findIndex((s) => s.id === id);
  const target = index + delta;
  if (index < 0 || target < 0 || target >= content.sections.length) return content;
  const sections = [...content.sections];
  const [moved] = sections.splice(index, 1);
  if (moved) sections.splice(target, 0, moved);
  return { ...content, sections };
}

export function removeSection(content: PortfolioContent, id: string): PortfolioContent {
  return { ...content, sections: content.sections.filter((s) => s.id !== id) };
}

export function addSection(content: PortfolioContent, kind: SectionKind): PortfolioContent {
  return { ...content, sections: [...content.sections, newSection(kind)] };
}

export function moveEntry(section: PortfolioSection, id: string, delta: -1 | 1): PortfolioSection {
  const index = section.entries.findIndex((e) => e.id === id);
  const target = index + delta;
  if (index < 0 || target < 0 || target >= section.entries.length) return section;
  const entries = [...section.entries];
  const [moved] = entries.splice(index, 1);
  if (moved) entries.splice(target, 0, moved);
  return { ...section, entries };
}
