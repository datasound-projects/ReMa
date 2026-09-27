import {
  commands,
  type BusinessProfile,
  type ClientSearchInput,
  type ContractSearchInput,
  type DescribeInput,
  type DraftRequest,
  type ExperimentContent,
  type ExperimentStatus,
  type ManualOpportunity,
  type NewActivity,
  type NewExperiment,
  type NewPlan,
  type OfferContent,
  type OpportunityEdit,
  type PlanPart,
  type PlanUpdate,
  type StageChange,
} from '../generated/bindings';
import { callBackend } from './ipc';

export type {
  Activity,
  ActivityType,
  Alternative,
  Amount,
  Assignment,
  Authority,
  BusinessOverview,
  BusinessProfile,
  BusinessRun,
  BuyerContact,
  ChannelPlan,
  Claim,
  ClientCriteria,
  ClientProspect,
  ClientResults,
  CohortEntry,
  Comparison,
  ContactRef,
  ContractCriteria,
  ContractResult,
  ContractResults,
  ContractTerms,
  CriterionScore,
  DescribeInput,
  DescribeResult,
  Draft,
  Engagement,
  Experiment,
  ExperimentContent,
  ExperimentMetrics,
  ExperimentStatus,
  FieldStatus,
  FitAssessment,
  GtmPlan,
  HardCheck,
  IngestPage,
  Locations,
  MatchStatus,
  Maturity,
  Offer,
  OfferContent,
  OfferDiff,
  OfferKind,
  OfferRef,
  Opportunity,
  OpportunityKind,
  PageStatus,
  Pipeline,
  PipelineStage,
  PlanContent,
  PlanPart,
  PricePoint,
  PriceUnit,
  Rate,
  RateUnit,
  Redaction,
  RunStatus,
  Segment,
  SegmentStatus,
  SourceNote,
  Suppression,
  TargetAccount,
  Variant,
  VariantMetrics,
} from '../generated/bindings';

/** A request id for one user action: a retry reuses it. */
export function requestKey(prefix = 'b'): string {
  const random =
    typeof crypto !== 'undefined' && 'randomUUID' in crypto
      ? crypto.randomUUID()
      : `${Date.now()}-${Math.random().toString(16).slice(2)}`;
  return `${prefix}-${random}`;
}

// ── Profile and offers ──────────────────────────────────────────────

export const getBusinessOverview = () => callBackend(() => commands.businessOverview());

export const saveBusinessProfile = (profile: BusinessProfile) =>
  callBackend(() => commands.businessSaveProfile(profile));

export const createOffer = (content: OfferContent, idempotencyKey: string) =>
  callBackend(() => commands.businessCreateOffer(content, idempotencyKey));

export const saveOfferDraft = (offerId: string, content: OfferContent, expectedRevision: number) =>
  callBackend(() => commands.businessSaveOfferDraft(offerId, content, expectedRevision));

/** Saves the draft as the next reviewed, immutable version. */
export const reviewOffer = (offerId: string, expectedRevision: number) =>
  callBackend(() => commands.businessReviewOffer(offerId, expectedRevision));

export const archiveOffer = (offerId: string, archived: boolean, expectedRevision: number) =>
  callBackend(() => commands.businessArchiveOffer(offerId, archived, expectedRevision));

export const deleteOffer = (offerId: string) => callBackend(() => commands.businessDeleteOffer(offerId));

export const getOfferVersion = (offerId: string, version: number) =>
  callBackend(() => commands.businessOfferVersion(offerId, version));

/** A product URL or description → a draft (or a refresh proposal). */
export const describeOffer = (input: DescribeInput) =>
  callBackend(() => commands.businessDescribeOffer(input));

/** Asks for a document with the system file dialog (null: cancelled). */
export const describeOfferFromDocument = (input: DescribeInput) =>
  callBackend(() => commands.businessDescribeOfferDocument(input));

// ── Research ────────────────────────────────────────────────────────

export const findClients = (input: ClientSearchInput) => callBackend(() => commands.businessFindClients(input));

export const findContracts = (input: ContractSearchInput) =>
  callBackend(() => commands.businessFindContracts(input));

export const cancelBusinessRun = (runId: string) => callBackend(() => commands.businessCancel(runId));

export const getLastBusinessResults = () => callBackend(() => commands.businessLastResults());

export const getBusinessRuns = (limit = 50) => callBackend(() => commands.businessRuns(limit));

// ── Pipeline ────────────────────────────────────────────────────────

export const getPipeline = () => callBackend(() => commands.businessPipeline());

export const saveProspect = (runId: string, companyKey: string, useCase: string | null) =>
  callBackend(() => commands.businessSaveProspect(runId, companyKey, useCase));

export const saveContract = (runId: string, key: string) =>
  callBackend(() => commands.businessSaveContract(runId, key));

export const createOpportunity = (input: ManualOpportunity) =>
  callBackend(() => commands.businessCreateOpportunity(input));

export const editOpportunity = (id: string, edit: OpportunityEdit, expectedRevision: number) =>
  callBackend(() => commands.businessEditOpportunity(id, edit, expectedRevision));

export const changeStage = (id: string, change: StageChange) =>
  callBackend(() => commands.businessChangeStage(id, change));

export const recordActivity = (id: string, activity: NewActivity) =>
  callBackend(() => commands.businessRecordActivity(id, activity));

export const deleteActivity = (id: string, activityId: string) =>
  callBackend(() => commands.businessDeleteActivity(id, activityId));

export const doNotContact = (id: string, reason: string | null) =>
  callBackend(() => commands.businessDoNotContact(id, reason));

export const suppressContact = (id: string, contactId: string, reason: string | null) =>
  callBackend(() => commands.businessSuppressContact(id, contactId, reason));

/** Lifts a Do-not-contact entry: only ever the user's own action. */
export const liftSuppression = (suppressionId: string) =>
  callBackend(() => commands.businessLiftSuppression(suppressionId));

export const deleteContact = (id: string, contactId: string, expectedRevision: number) =>
  callBackend(() => commands.businessDeleteContact(id, contactId, expectedRevision));

export const deleteOpportunity = (id: string) => callBackend(() => commands.businessDeleteOpportunity(id));

export const refreshListing = (id: string) => callBackend(() => commands.businessRefreshListing(id));

export const reassessOpportunity = (id: string, runId: string, offerVersion: number | null) =>
  callBackend(() => commands.businessReassess(id, runId, offerVersion));

export const getRedactions = () => callBackend(() => commands.businessRedactions());

// ── Go-to-Market Studio ─────────────────────────────────────────────

export const createPlan = (input: NewPlan) => callBackend(() => commands.businessCreatePlan(input));

export const savePlan = (id: string, update: PlanUpdate) => callBackend(() => commands.businessSavePlan(id, update));

export const deletePlan = (id: string) => callBackend(() => commands.businessDeletePlan(id));

export const researchPlan = (id: string, part: PlanPart, runId: string) =>
  callBackend(() => commands.businessResearchPlan(id, part, runId));

export const findTargetAccounts = (planId: string, segmentId: string, runId: string) =>
  callBackend(() => commands.businessTargetAccounts(planId, segmentId, runId));

export const draftPositioning = (planId: string, segmentId: string, alternative: number | null) =>
  callBackend(() => commands.businessPositioning(planId, segmentId, alternative));

/** A local draft. ReMa has no sending action. */
export const createDraft = (request: DraftRequest, runId: string) =>
  callBackend(() => commands.businessCreateDraft(request, runId));

export const updateDraft = (id: string, subject: string | null, body: string, expectedRevision: number) =>
  callBackend(() => commands.businessUpdateDraft(id, subject, body, expectedRevision));

export const deleteDraft = (id: string) => callBackend(() => commands.businessDeleteDraft(id));

export const createExperiment = (input: NewExperiment) => callBackend(() => commands.businessCreateExperiment(input));

export const updateExperiment = (id: string, content: ExperimentContent, expectedRevision: number) =>
  callBackend(() => commands.businessUpdateExperiment(id, content, expectedRevision));

export const setExperimentStatus = (id: string, status: ExperimentStatus, expectedRevision: number) =>
  callBackend(() => commands.businessSetExperimentStatus(id, status, expectedRevision));

export const amendExperiment = (id: string, idempotencyKey: string) =>
  callBackend(() => commands.businessAmendExperiment(id, idempotencyKey));

export const deleteExperiment = (id: string) => callBackend(() => commands.businessDeleteExperiment(id));

export const getExperimentMetrics = (id: string) => callBackend(() => commands.businessExperimentMetrics(id));
