import type {
  Claim,
  ContractCriteria,
  ContractTerms,
  Locations,
  OfferContent,
  OfferKind,
  RateUnit,
} from '../../services/businessService';
import { openExternalUrl } from '../../services/systemService';

/** External links open in the system browser, only when clicked (B27). */
export function openLink(url: string | null | undefined) {
  if (url) void openExternalUrl(url).catch(() => {});
}

/** Copies text only: never a contact, a stage change or a count (B21). */
export async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    return false;
  }
}

export const EMPTY_LOCATIONS: Locations = { countries: [], regions: [], cities: [], radiusKm: null };

export function isEmptyLocations(l: Locations) {
  return l.countries.length === 0 && l.regions.length === 0 && l.cities.length === 0;
}

/** "3 / 20 (15%)" or "Not available" for a zero denominator (B23). */
export function rateText(r: { numerator: number; denominator: number; display: string }) {
  return r.denominator === 0 ? 'Not available' : r.display;
}

/** Date input value (local) ↔ epoch ms. */
export function toDateInput(ms: number | null): string {
  if (ms === null) return '';
  const d = new Date(ms);
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

export function fromDateInput(value: string): number | null {
  if (!value) return null;
  const [y, m, d] = value.split('-').map(Number);
  if (!y || !m || !d) return null;
  return new Date(y, m - 1, d, 12, 0, 0).getTime();
}

/**
 * When something happened, from a date input: today means now (so an
 * activity recorded today is never placed before an experiment started
 * today); an earlier day means midday of that day.
 */
export function occurredAt(value: string): number {
  const now = Date.now();
  if (!value || value === toDateInput(now)) return now;
  return Math.min(fromDateInput(value) ?? now, now);
}

export const RATE_UNITS: Record<RateUnit, string> = {
  hour: 'per hour',
  day: 'per day',
  month: 'per month',
  project: 'per project',
};

export const DEFAULT_CONTRACT_CRITERIA: ContractCriteria = {
  skills: '',
  locations: EMPTY_LOCATIONS,
  remoteOk: true,
  minRate: null,
  rateComparison: 'above',
  currency: 'EUR',
  rateUnit: 'day',
  hoursPerDay: null,
  durationMinMonths: null,
  durationMaxMonths: null,
  postedWithinDays: null,
};

export function rateSummary(t: ContractTerms): string {
  if (t.rateText) return t.rateText;
  if (t.rateMin === null && t.rateMax === null) return 'Not stated';
  const unit = t.rateUnit ? ` ${RATE_UNITS[t.rateUnit]}` : '';
  const cur = t.currency ? `${t.currency} ` : '';
  if (t.rateIsCeiling || t.rateMin === null) return `Up to ${cur}${t.rateMax}${unit}`;
  if (t.rateMax === null || t.rateMax === t.rateMin) return `${cur}${t.rateMin}${unit}`;
  return `${cur}${t.rateMin}–${t.rateMax}${unit}`;
}

export function durationSummary(t: ContractTerms): string {
  const parts: string[] = [];
  if (t.durationText) parts.push(t.durationText);
  else if (t.durationMin !== null || t.durationMax !== null) {
    const unit = t.durationUnit === 'week' ? 'weeks' : 'months';
    parts.push(
      t.durationMin !== null && t.durationMax !== null && t.durationMin !== t.durationMax
        ? `${t.durationMin}–${t.durationMax} ${unit}`
        : `${t.durationMin ?? t.durationMax} ${unit}`,
    );
  } else parts.push('Duration not stated');
  if (t.extensionPossible) parts.push('extension possible');
  if (t.start) parts.push(`start ${t.start}`);
  return parts.join(' · ');
}

export const CHANNELS = ['Email', 'LinkedIn message', 'Contact form', 'Phone call notes'];

export function isKnownClaim(c: Claim) {
  return c.status !== 'unknown' && c.text.trim() !== '';
}

/** Every claim of an offer: the single fields, the lists and the prices. */
export function allClaims(content: OfferContent): Claim[] {
  return [
    content.summary,
    content.problem,
    content.deliveryModel,
    ...content.outcomes,
    ...content.features,
    ...content.useCases,
    ...content.customerTypes,
    ...content.buyerRoles,
    ...content.geography,
    ...content.languages,
    ...content.requirements,
    ...content.integrations,
    ...content.deploymentConstraints,
    ...content.exclusions,
    ...content.limitations,
    ...content.pricing.map((p) => p.claim),
  ];
}

/** The same checks the backend applies before a version can be saved (B3). */
export function reviewBlockers(content: OfferContent): string[] {
  const out: string[] = [];
  if (!content.name.trim()) out.push('Give the offer a name.');
  if (!isKnownClaim(content.summary)) out.push('Describe what it does.');
  if (!isKnownClaim(content.problem)) out.push('Describe the problem it addresses.');
  const conflicting = allClaims(content).filter((c) => c.status === 'conflicting').length;
  if (conflicting > 0) {
    out.push(`Resolve ${conflicting} conflicting claim${conflicting === 1 ? '' : 's'} (keep, correct or reject).`);
  }
  return out;
}

/** An offer with nothing stated yet: every field unknown. */
export function emptyOfferContent(name: string, kind: OfferKind): OfferContent {
  const unknown = (): Claim => ({ text: '', status: 'unknown', sourceUrl: null, retrievedAt: null, excerpt: null, note: null });
  return {
    name,
    kind,
    maturity: 'not_stated',
    summary: unknown(),
    problem: unknown(),
    outcomes: [],
    features: [],
    useCases: [],
    customerTypes: [],
    buyerRoles: [],
    pricing: [],
    deliveryModel: unknown(),
    geography: [],
    languages: [],
    requirements: [],
    integrations: [],
    deploymentConstraints: [],
    exclusions: [],
    unsupportedClaims: [],
    limitations: [],
    websiteUrls: [],
  };
}
