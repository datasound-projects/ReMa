import type {
  ActivityType,
  Assignment,
  Authority,
  Engagement,
  ExperimentStatus,
  FieldStatus,
  MatchStatus,
  Maturity,
  OfferKind,
  OpportunityKind,
  PageStatus,
  PipelineStage,
  PriceUnit,
  RunStatus,
  SegmentStatus,
} from '../../services/businessService';

export const OFFER_KIND_LABELS: Record<OfferKind, string> = {
  service: 'Service',
  digital_product: 'Digital product',
  hybrid: 'Service and product',
};

export const MATURITY_LABELS: Record<Maturity, string> = {
  not_stated: 'Not stated',
  prototype: 'Prototype',
  pilot_ready: 'Pilot-ready',
  generally_available: 'Generally available',
};

export const FIELD_STATUS_LABELS: Record<FieldStatus, string> = {
  observed: 'From the source — not confirmed',
  user_confirmed: 'Confirmed by you',
  hypothesis: 'Hypothesis',
  unknown: 'Unknown',
  conflicting: 'Conflicting',
};

export const PRICE_UNIT_LABELS: Record<PriceUnit, string> = {
  hourly: 'per hour',
  daily: 'per day',
  project: 'per project',
  seat_month: 'per seat per month',
  organization_month: 'per organization per month',
  annual: 'per year',
  one_time: 'one-time',
  custom_quote: 'custom quote',
};

export const PAGE_STATUS_LABELS: Record<PageStatus, string> = {
  read: 'Read',
  truncated: 'Read up to the size limit',
  blocked: 'Not allowed',
  failed: 'Could not be read',
  out_of_scope: 'Outside the product site',
};

export const RUN_STATUS_LABELS: Record<RunStatus, string> = {
  queued: 'Queued',
  running: 'Running',
  complete: 'Complete',
  partial: 'Partial: some sources could not be searched',
  no_verified_matches: 'No verified matches (the search ran)',
  needs_review: 'Needs your review',
  capability_unavailable: 'Not available with the current setup',
  offline: 'Offline: no source could be reached',
  failed: 'Failed: no source could be searched',
  cancelled: 'Stopped',
};

export function runStatusTone(status: RunStatus): 'success' | 'warning' | 'danger' | 'neutral' {
  switch (status) {
    case 'complete':
      return 'success';
    case 'partial':
    case 'needs_review':
    case 'no_verified_matches':
      return 'warning';
    case 'failed':
    case 'offline':
    case 'capability_unavailable':
      return 'danger';
    default:
      return 'neutral';
  }
}

export const STAGE_LABELS: Record<PipelineStage, string> = {
  new_lead: 'New Lead',
  qualified: 'Qualified',
  contacted: 'Contacted',
  discussion: 'Discussion',
  proposal: 'Proposal',
  won: 'Won',
  lost: 'Lost',
};

export const STAGES: PipelineStage[] = [
  'new_lead',
  'qualified',
  'contacted',
  'discussion',
  'proposal',
  'won',
  'lost',
];

/** What each stage means (B15): shown where the stage is changed. */
export const STAGE_MEANINGS: Record<PipelineStage, string> = {
  new_lead: 'You saved a candidate; no buying intent is implied.',
  qualified: 'You confirmed the fit for further commercial work.',
  contacted: 'You actually reached out (a draft or an opened link does not count).',
  discussion: 'A real exchange or meeting happened.',
  proposal: 'You actually submitted a proposal (a draft does not count).',
  won: 'An engagement or order was accepted (not money received).',
  lost: 'Closed, declined or not a fit.',
};

export const OPPORTUNITY_KIND_LABELS: Record<OpportunityKind, string> = {
  product: 'Product',
  service: 'Service',
  contract: 'Contract',
};

export const ACTIVITY_LABELS: Record<ActivityType, string> = {
  contact: 'Contacted',
  reply: 'Reply received',
  positive_reply: 'Positive reply',
  meeting_held: 'Meeting held',
  proposal_sent: 'Proposal sent',
  won: 'Won',
  lost: 'Lost',
  stage_change: 'Stage changed',
  note: 'Note',
};

export const RECORDABLE: ActivityType[] = [
  'contact',
  'reply',
  'positive_reply',
  'meeting_held',
  'proposal_sent',
  'won',
  'lost',
  'note',
];

export const AUTHORITY_LABELS: Record<Authority, string> = {
  verified_responsibility: 'Stated responsibility',
  likely_functional_contact: 'Likely functional contact',
  unknown_authority: 'Authority unknown',
};

export const ENGAGEMENT_LABELS: Record<Engagement, string> = {
  freelance: 'Freelance',
  contract: 'Contract',
  b2b: 'B2B',
  interim: 'Interim',
  project: 'Project',
  fixed_term_employee: 'Fixed-term employment',
  permanent_employee: 'Permanent employment',
  employment: 'Employment (term not stated)',
  needs_verification: 'Engagement type needs verification',
};

export const MATCH_LABELS: Record<MatchStatus, string> = {
  confirmed: 'Confirmed',
  needs_verification: 'Needs verification',
  not_matching: 'Not matching',
};

export const SEGMENT_STATUS_LABELS: Record<SegmentStatus, string> = {
  hypothesis: 'Hypothesis',
  under_test: 'Under test',
  supported_by_observations: 'Supported by observations',
  rejected: 'Rejected',
};

export const EXPERIMENT_STATUS_LABELS: Record<ExperimentStatus, string> = {
  draft: 'Draft',
  planned: 'Planned',
  running: 'Running',
  paused: 'Paused',
  completed: 'Completed',
  cancelled: 'Cancelled',
};

export const ASSIGNMENT_LABELS: Record<Assignment, string> = {
  random_by_account: 'Randomized by account (frozen before contact)',
  manual: 'Chosen by you (observational)',
  sequential: 'One variant after another (observational)',
};

/** "72 (evidence coverage 80%)" or "Insufficient evidence (coverage 35%)". */
export function fitLabel(score: number | null, coverage: number, shown: boolean): string {
  const pct = `${Math.round(coverage * 100)}%`;
  return shown && score !== null
    ? `${Math.round(score)} (evidence coverage ${pct})`
    : `Insufficient evidence (coverage ${pct})`;
}
