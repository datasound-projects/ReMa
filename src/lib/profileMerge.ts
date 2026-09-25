import type { Education, Experience, Language, Profile, ProfileLink } from '../services/profileService';

/** Single-value fields an import can propose. */
export const SCALAR_FIELDS = [
  ['firstName', 'First name'],
  ['lastName', 'Last name'],
  ['email', 'Email'],
  ['phone', 'Phone'],
  ['location', 'Location'],
  ['title', 'Professional title'],
  ['summary', 'Summary'],
  ['website', 'Website'],
  ['resumeWebsite', 'Resume website'],
  ['github', 'GitHub'],
  ['linkedin', 'LinkedIn'],
] as const;

export type ScalarField = (typeof SCALAR_FIELDS)[number][0];

/** One proposed change, shown as a checkbox in the review. */
export interface ReviewItem {
  id: string;
  section: string;
  label: string;
  value: string;
  /** The value it would replace, if different and not empty. */
  replaces: string | null;
  /** Checked by default: fills a gap; unchecked: would replace something. */
  selected: boolean;
}

const same = (a: string, b: string) => a.trim().toLowerCase() === b.trim().toLowerCase();

const experienceKey = (e: Experience) => `${e.title}|${e.company}`.toLowerCase();
const educationKey = (e: Education) => `${e.degree}|${e.school}`.toLowerCase();
const languageKey = (l: Language) => l.name.toLowerCase();
const linkKey = (l: ProfileLink) => l.url.toLowerCase().replace(/\/$/, '');

const period = (start: string, end: string, current = false) =>
  [start, current ? 'present' : end].filter(Boolean).join(' – ');

/** What an import would change. Existing values are never replaced by default. */
export function reviewItems(current: Profile, extracted: Profile): ReviewItem[] {
  const items: ReviewItem[] = [];
  for (const [field, label] of SCALAR_FIELDS) {
    const value = extracted[field].trim();
    const existing = current[field].trim();
    if (!value || same(value, existing)) continue;
    items.push({
      id: `field:${field}`,
      section: 'Details',
      label,
      value,
      replaces: existing || null,
      selected: !existing,
    });
  }

  const newSkills = extracted.skills.filter((s) => !current.skills.some((c) => same(c, s)));
  if (newSkills.length > 0) {
    items.push({
      id: 'skills',
      section: 'Skills',
      label: newSkills.length === 1 ? 'Add 1 skill' : `Add ${newSkills.length} skills`,
      value: newSkills.join(', '),
      replaces: null,
      selected: true,
    });
  }

  const known = new Set(current.experience.map(experienceKey));
  extracted.experience.forEach((e, i) => {
    if (known.has(experienceKey(e))) return;
    items.push({
      id: `experience:${i}`,
      section: 'Experience',
      label: [e.title, e.company].filter(Boolean).join(' · ') || 'Position',
      value: period(e.start, e.end, e.current),
      replaces: null,
      selected: true,
    });
  });

  const knownEducation = new Set(current.education.map(educationKey));
  extracted.education.forEach((e, i) => {
    if (knownEducation.has(educationKey(e))) return;
    items.push({
      id: `education:${i}`,
      section: 'Education',
      label: [e.degree, e.field, e.school].filter(Boolean).join(' · ') || 'Education',
      value: period(e.start, e.end),
      replaces: null,
      selected: true,
    });
  });

  const knownLanguages = new Set(current.languages.map(languageKey));
  extracted.languages.forEach((l, i) => {
    if (knownLanguages.has(languageKey(l))) return;
    items.push({
      id: `language:${i}`,
      section: 'Languages',
      label: l.name,
      value: l.level,
      replaces: null,
      selected: true,
    });
  });

  const knownLinks = new Set(current.otherLinks.map(linkKey));
  extracted.otherLinks.forEach((l, i) => {
    if (knownLinks.has(linkKey(l))) return;
    items.push({
      id: `link:${i}`,
      section: 'Links',
      label: l.label || 'Link',
      value: l.url,
      replaces: null,
      selected: true,
    });
  });
  return items;
}

/** The profile with the selected changes applied. */
export function applyReview(current: Profile, extracted: Profile, selected: Set<string>): Profile {
  const next: Profile = structuredClone(current);
  for (const [field] of SCALAR_FIELDS) {
    if (selected.has(`field:${field}`)) next[field] = extracted[field];
  }
  if (selected.has('skills')) {
    next.skills = [...next.skills, ...extracted.skills.filter((s) => !next.skills.some((c) => same(c, s)))];
  }
  extracted.experience.forEach((e, i) => selected.has(`experience:${i}`) && next.experience.push(e));
  extracted.education.forEach((e, i) => selected.has(`education:${i}`) && next.education.push(e));
  extracted.languages.forEach((l, i) => selected.has(`language:${i}`) && next.languages.push(l));
  extracted.otherLinks.forEach((l, i) => selected.has(`link:${i}`) && next.otherLinks.push(l));
  return next;
}

// ── Small list helpers for the editor ──────────────────────────────

export function replaceAt<T>(list: T[], index: number, item: T): T[] {
  return list.map((x, i) => (i === index ? item : x));
}

export function removeAt<T>(list: T[], index: number): T[] {
  return list.filter((_, i) => i !== index);
}

/** Moves an item up (-1) or down (+1). */
export function move<T>(list: T[], index: number, delta: -1 | 1): T[] {
  const target = index + delta;
  if (target < 0 || target >= list.length) return list;
  const next = [...list];
  [next[index], next[target]] = [next[target] as T, next[index] as T];
  return next;
}
