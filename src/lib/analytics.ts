/**
 * Display labels and formatting for the Analytics dashboard. Only
 * presentation lives here: every number is calculated in Rust.
 */
import type {
  DataScope,
  JobSearchRun,
  SortCriterion,
  EmploymentType,
  FilterCondition,
  FilterField,
  FilterOp,
  FrequencyClass,
  JobColumn,
  MatchState,
  PriorityLevel,
  RequirementCategory,
  ResourceType,
  Seniority,
  SortKey,
  WorkMode,
} from '../services/analyticsService';

export const WORK_MODES: Record<WorkMode, string> = {
  remote: 'Remote',
  hybrid: 'Hybrid',
  onsite: 'On-site',
};

export const EMPLOYMENT_TYPES: Record<EmploymentType, string> = {
  full_time: 'Full-time',
  part_time: 'Part-time',
  contract: 'Contract',
  freelance: 'Freelance',
  temporary: 'Temporary',
  internship: 'Internship',
};

export const SENIORITIES: Record<Seniority, string> = {
  intern: 'Intern',
  entry: 'Entry / junior',
  mid: 'Mid-level',
  senior: 'Senior',
  lead: 'Lead / staff',
  executive: 'Head / director',
};

export const CATEGORIES: Record<RequirementCategory, string> = {
  technical_skills: 'Technical skills',
  programming_languages: 'Programming languages',
  frameworks: 'Frameworks',
  cloud_infrastructure: 'Cloud / infrastructure',
  ai_ml: 'AI / ML',
  data_engineering: 'Data engineering',
  databases: 'Databases',
  professional_experience: 'Professional experience',
  industry_experience: 'Industry experience',
  education: 'Education',
  certifications: 'Certifications',
  languages: 'Languages',
  soft_skills: 'Soft skills',
  other: 'Other requirements',
};

export const STATES: Record<MatchState, string> = {
  matched: 'Matched',
  partial: 'Partial',
  missing: 'Missing',
  unknown: 'Unknown',
};

/** State glyphs: identity never rests on color alone. */
export const STATE_ICONS: Record<MatchState, string> = {
  matched: '✓',
  partial: '◐',
  missing: '✕',
  unknown: '?',
};

export const CLASSES: Record<FrequencyClass, string> = {
  very_common: 'Very common',
  common: 'Common',
  occasional: 'Occasional',
  rare: 'Rare / unique',
};

export const PRIORITIES: Record<PriorityLevel, string> = {
  high: 'High',
  medium: 'Medium',
  low: 'Low',
};

export const RESOURCE_TYPES: Record<ResourceType, string> = {
  certification: 'Certification',
  course: 'Course',
  university: 'University course',
  documentation: 'Documentation',
  book: 'Book',
  lab: 'Practical lab',
  tutorial: 'Tutorial',
  project: 'Project',
  program: 'Training program',
};

export const COLUMNS: Record<JobColumn, string> = {
  rank: 'Rank',
  company: 'Company',
  role: 'Role',
  location: 'Location',
  work_mode: 'Work mode',
  salary: 'Salary',
  seniority: 'Seniority',
  match: 'Match',
  skill_gap: 'Skill gap',
  posted: 'Posted',
  discovered: 'Found',
  source: 'Source',
};

export const FIELDS: Record<FilterField, string> = {
  country: 'Country',
  city: 'City',
  work_mode: 'Work mode',
  salary: 'Salary',
  date_posted: 'Date posted',
  date_discovered: 'Date found',
  role: 'Role',
  title: 'Job title',
  company: 'Company',
  seniority: 'Seniority',
  employment_type: 'Employment type',
  skill: 'Required skill',
  missing_skill: 'Skill missing from Profile',
  match: 'Profile match',
  skill_gap: 'Missing skills (count)',
  language: 'Language',
  certification: 'Certification',
  location: 'Location text',
  source: 'Source',
  search_run: 'Search',
};

export const OPS: Record<FilterOp, string> = {
  is: 'is',
  is_not: 'is not',
  contains: 'contains',
  not_contains: 'does not contain',
  at_least: 'at least',
  at_most: 'at most',
  within_days: 'in the last',
  older_than_days: 'more than … ago',
  on_or_after: 'on or after',
  before: 'before',
  has_any: 'any of',
  has_all: 'all of',
  has_none: 'none of',
  known: 'is known',
  unknown: 'is unknown',
};

/** Operators offered per field (Rust validates the same rules). */
export const FIELD_OPS: Record<FilterField, FilterOp[]> = {
  company: ['is', 'is_not', 'contains', 'not_contains', 'known', 'unknown'],
  title: ['contains', 'not_contains', 'is', 'is_not'],
  role: ['is', 'is_not', 'contains', 'not_contains'],
  location: ['contains', 'not_contains', 'known', 'unknown'],
  source: ['is', 'is_not', 'contains', 'known', 'unknown'],
  country: ['is', 'is_not', 'known', 'unknown'],
  city: ['is', 'is_not', 'known', 'unknown'],
  work_mode: ['is', 'is_not', 'known', 'unknown'],
  employment_type: ['is', 'is_not', 'known', 'unknown'],
  seniority: ['is', 'is_not', 'known', 'unknown'],
  salary: ['at_least', 'at_most', 'known', 'unknown'],
  date_posted: ['within_days', 'older_than_days', 'on_or_after', 'before', 'known', 'unknown'],
  date_discovered: ['within_days', 'older_than_days', 'on_or_after', 'before'],
  skill: ['has_any', 'has_all', 'has_none'],
  missing_skill: ['has_any', 'has_none'],
  match: ['at_least', 'at_most'],
  skill_gap: ['at_most', 'at_least'],
  language: ['has_any', 'has_all', 'has_none'],
  certification: ['has_any', 'has_none', 'known', 'unknown'],
  search_run: ['has_any', 'has_none'],
};

export const RANKING_KEYS: Record<SortKey, string> = {
  work_mode: 'Remote first',
  salary: 'Salary',
  match: 'Profile match',
  skill_gap: 'Skill gap (missing)',
  date_posted: 'Date posted',
  date_discovered: 'Date found',
  company: 'Company',
  title: 'Job title',
  seniority: 'Seniority',
  prefer_country: 'Country first',
  prefer_city: 'City first',
  prefer_role: 'Role first',
  prefer_company: 'Company first',
  prefer_skill: 'Requiring a skill first',
};

export const PREFER_KEYS: SortKey[] = [
  'prefer_country',
  'prefer_city',
  'prefer_role',
  'prefer_company',
  'prefer_skill',
];

/** Direction labels per key: what "descending" means to a reader. */
export function directionLabel(key: SortKey, descending: boolean): string {
  switch (key) {
    case 'work_mode':
      return descending ? 'remote → on-site' : 'on-site → remote';
    case 'salary':
      return descending ? 'highest first' : 'lowest first';
    case 'match':
      return descending ? 'best first' : 'lowest first';
    case 'skill_gap':
      return descending ? 'most missing first' : 'fewest missing first';
    case 'date_posted':
    case 'date_discovered':
      return descending ? 'newest first' : 'oldest first';
    case 'seniority':
      return descending ? 'most senior first' : 'most junior first';
    case 'company':
    case 'title':
      return descending ? 'Z → A' : 'A → Z';
    default:
      return descending ? 'first' : 'last';
  }
}

export const ALL_SCOPE: DataScope = { kind: 'all', runIds: [], jobIds: [] };

/** A calendar day stored as UTC midnight, e.g. "20 Sep 2025". */
export function formatDay(ms: number | null | undefined): string {
  if (ms == null) return '—';
  return new Date(ms).toLocaleDateString(undefined, {
    day: 'numeric',
    month: 'short',
    year: 'numeric',
    timeZone: 'UTC',
  });
}

export function formatDateTime(ms: number): string {
  return new Date(ms).toLocaleString(undefined, {
    day: 'numeric',
    month: 'short',
    hour: '2-digit',
    minute: '2-digit',
  });
}

/** "1 job", "3 jobs". */
export function jobsText(count: number): string {
  return `${count} job${count === 1 ? '' : 's'}`;
}

export function formatPercent(value: number | null | undefined): string {
  return value == null ? '—' : `${Math.round(value)}%`;
}

export function emptyCondition(field: FilterField): FilterCondition {
  return { field, op: FIELD_OPS[field][0] ?? 'is', values: [], number: null, currency: null };
}

/** Whether a condition is complete enough to send. */
export function conditionReady(c: FilterCondition): boolean {
  const values = (c.values ?? []).filter((v) => v.trim() !== '');
  switch (c.op) {
    case 'known':
    case 'unknown':
      return true;
    case 'at_least':
    case 'at_most':
    case 'within_days':
    case 'older_than_days':
      return c.number != null && Number.isFinite(c.number) && c.number >= 0;
    case 'on_or_after':
    case 'before':
      return /^\d{4}-\d{2}-\d{2}$/.test(values[0] ?? '');
    default:
      return values.length > 0;
  }
}

export const ENUMS: Partial<Record<FilterField, Record<string, string>>> = {
  work_mode: WORK_MODES,
  employment_type: EMPLOYMENT_TYPES,
  seniority: SENIORITIES,
};

/** Short text for a condition chip. */
export function describeCondition(c: FilterCondition, runs: JobSearchRun[]): string {
  const values = c.values ?? [];
  const field = FIELDS[c.field];
  const shown =
    c.field === 'search_run'
      ? values.map((v) => runs.find((r) => String(r.id) === v)?.title ?? `#${v}`)
      : ENUMS[c.field]
        ? values.map((v) => ENUMS[c.field]?.[v] ?? (v === 'unknown' ? 'Unknown' : v))
        : values;
  const n = c.number ?? 0;
  switch (c.op) {
    case 'known':
    case 'unknown':
      return `${field} ${OPS[c.op]}`;
    case 'within_days':
      return `${field}: last ${n === 1 ? '24 hours' : `${n} days`}`;
    case 'older_than_days':
      return `${field}: more than ${n} days ago`;
    case 'at_least':
    case 'at_most': {
      const sign = c.op === 'at_least' ? '≥' : '≤';
      if (c.field === 'salary') return `Salary ${sign} ${c.currency ? `${c.currency} ` : ''}${n.toLocaleString()} / year`;
      if (c.field === 'match') return `Profile match ${sign} ${n}%`;
      return `${field} ${sign} ${n}`;
    }
    case 'has_any':
      return `${field}: ${shown.join(' or ')}`;
    case 'has_all':
      return `${field}: ${shown.join(' and ')}`;
    case 'has_none':
      return `${field}: none of ${shown.join(', ')}`;
    default:
      return `${field} ${OPS[c.op]} ${shown.join(' or ')}`;
  }
}

export const DEFAULT_RANKING: SortCriterion[] = [
  { key: 'date_posted', direction: 'desc', value: null },
  { key: 'date_discovered', direction: 'desc', value: null },
];


/** Whether a model answer probably lists jobs (a table, or several links). */
export function mayListJobs(text: string): boolean {
  const table = /^\s*\|?\s*:?-{3,}/m.test(text) && text.includes('|');
  // Links to search result pages are suggestions, not postings.
  const links = (text.match(/https?:\/\/[^\s)\]>"']+/g) ?? []).filter((url) => !isSearchPage(url));
  return table || links.length >= 2;
}

/** A job board's search results page rather than a posting. */
export function isSearchPage(url: string): boolean {
  return /[?&](q|query|keywords?|search|k|kw|what|text)=|\/(search|suche|jobs\/search|jobsuche)(\/|\?|$)/i.test(url);
}
