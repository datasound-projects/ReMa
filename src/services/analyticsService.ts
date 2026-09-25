import {
  commands,
  type AnalyticsOverview,
  type AnalyticsPreferences,
  type AnalyticsQuery,
  type JobSearchRun,
  type LearningCriteria,
  type LearningView,
  type LinkedRun,
  type RequirementTableQuery,
  type RequirementsView,
  type SkillGapOptions,
  type SkillGapView,
} from '../generated/bindings';
import { callBackend } from './ipc';

export type {
  AnalyticsOverview,
  AnalyticsPreferences,
  AnalyticsQuery,
  CellState,
  DashboardTab,
  DataScope,
  EmploymentType,
  Facets,
  FacetValue,
  FilterCondition,
  FilterField,
  FilterOp,
  FrequencyClass,
  GapPriority,
  JobColumn,
  JobGap,
  JobSearchRun,
  LearningCriteria,
  LearningGap,
  LearningRecommendation,
  LearningResearch,
  LearningResource,
  LearningView,
  LinkedRun,
  MatchState,
  PipelineStatus,
  PriorityLevel,
  RankedJob,
  RequirementCategory,
  RequirementKind,
  RequirementRow,
  RequirementSort,
  RequirementTableQuery,
  RequirementsView,
  ResourceType,
  RunSource,
  ScopeKind,
  Seniority,
  SkillDemand,
  SkillGapOptions,
  SkillGapView,
  SortCriterion,
  SortDirection,
  SortKey,
  WorkMode,
} from '../generated/bindings';

export function getAnalyticsPreferences(): Promise<AnalyticsPreferences> {
  return callBackend(() => commands.getAnalyticsPreferences());
}

export function saveAnalyticsPreferences(preferences: AnalyticsPreferences): Promise<null> {
  return callBackend(() => commands.saveAnalyticsPreferences(preferences));
}

export function listJobSearchRuns(): Promise<JobSearchRun[]> {
  return callBackend(() => commands.listJobSearchRuns());
}

export function deleteJobSearchRun(id: number): Promise<null> {
  return callBackend(() => commands.deleteJobSearchRun(id));
}

export function getAnalyticsOverview(query: AnalyticsQuery): Promise<AnalyticsOverview> {
  return callBackend(() => commands.getAnalyticsOverview(query));
}

export function getSkillGap(query: AnalyticsQuery, options: SkillGapOptions): Promise<SkillGapView> {
  return callBackend(() => commands.getSkillGap(query, options));
}

export function getRequirementsAnalysis(
  query: AnalyticsQuery,
  table: RequirementTableQuery,
): Promise<RequirementsView> {
  return callBackend(() => commands.getRequirementsAnalysis(query, table));
}

export function getLearning(
  query: AnalyticsQuery,
  criteria: LearningCriteria,
  options: SkillGapOptions,
): Promise<LearningView> {
  return callBackend(() => commands.getLearning(query, criteria, options));
}

/** Starts learning-resource research in the background. */
export function researchLearning(
  query: AnalyticsQuery,
  criteria: LearningCriteria,
  options: SkillGapOptions,
): Promise<LearningView> {
  return callBackend(() => commands.researchLearning(query, criteria, options));
}

/** Makes a chat answer's jobs available to Analytics; resolves to the search run. */
export function analyzeAnswer(messageId: number): Promise<number> {
  return callBackend(() => commands.analyzeAnswer(messageId));
}

export function analyzeTaskResult(executionId: number): Promise<number> {
  return callBackend(() => commands.analyzeTaskResult(executionId));
}

export function listConversationJobRuns(conversationId: number): Promise<LinkedRun[]> {
  return callBackend(() => commands.listConversationJobRuns(conversationId));
}

export function listTaskJobRuns(taskId: number): Promise<LinkedRun[]> {
  return callBackend(() => commands.listTaskJobRuns(taskId));
}
