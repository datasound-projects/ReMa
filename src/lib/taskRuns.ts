import type { StatusTone } from '../components/ui/StatusIndicator';
import type {
  RunContext,
  RunErrorCategory,
  RunOutputKind,
  TaskRun,
  TaskRunSummary,
} from '../generated/bindings';
import { formatWhen } from './format';

type RunLike = Pick<TaskRunSummary, 'status' | 'errorCategory'>;

/** "Run — Today at 21:30": when the run was created (the firing, or Run now). */
export function runTitle(run: Pick<TaskRunSummary, 'queuedAt'>, now = new Date()): string {
  return `Run — ${formatWhen(run.queuedAt, now)}`;
}

/** The run's state as a status dot and label. */
export function runStatus(run: RunLike): { tone: StatusTone; label: string } {
  switch (run.status) {
    case 'queued':
      return { tone: 'pending', label: 'Queued' };
    case 'running':
      return { tone: 'pending', label: 'Running' };
    case 'succeeded':
      return { tone: 'ready', label: 'Succeeded' };
    case 'failed':
      return { tone: 'error', label: 'Failed' };
    case 'cancelled':
      return { tone: 'idle', label: run.errorCategory === 'skipped' ? 'Skipped' : 'Cancelled' };
  }
}

/** Small labels in the run list: "Manual", "Failed" (success needs none). */
export function runTags(run: RunLike & Pick<TaskRunSummary, 'trigger'>): string[] {
  const tags: string[] = [];
  if (run.trigger === 'manual') tags.push('Manual');
  if (run.status !== 'succeeded') tags.push(runStatus(run).label);
  return tags;
}

export function isActive(run: RunLike): boolean {
  return run.status === 'queued' || run.status === 'running';
}

/** What the user can do about a failed or stopped run. */
export interface Guidance {
  text: string;
  action?: 'connectors' | 'settings' | 'edit';
}

export function failureGuidance(category: RunErrorCategory | null): Guidance | null {
  switch (category) {
    case null:
      return null;
    case 'connector':
      return {
        text: 'A connected service needs attention. Go to Settings → Connectors.',
        action: 'connectors',
      };
    case 'model_access':
      return {
        text: "The task's model can't be reached with the current sign-in or key. Check Settings → Models & providers.",
        action: 'settings',
      };
    case 'billing':
      return { text: "Add credits in the provider's billing settings, or choose another model for the task." };
    case 'provider':
      return { text: 'The provider reported a problem. The next run tries again.' };
    case 'network':
      return { text: 'ReMa could not reach the network. The next run tries again.' };
    case 'timeout':
      return { text: 'The run took longer than 15 minutes and was stopped. Try Run now, or narrow the prompt.' };
    case 'interrupted':
      return { text: 'ReMa was closed or stopped while this run was in progress. The next run starts normally.' };
    case 'cancelled':
      return { text: 'This run was stopped before it finished.' };
    case 'skipped':
      return { text: 'The previous run was still in progress at this time, so this occurrence was skipped.' };
    case 'model':
      return { text: 'Rephrase the prompt or choose another model for the task.', action: 'edit' };
    case 'search':
      return { text: 'No search could run for this request, so no listings were presented.' };
    case 'task':
      return { text: "The task can't run as it is set up.", action: 'edit' };
    case 'internal':
      return { text: 'Something went wrong inside ReMa. Try Run now.' };
  }
}

/** "Enabled · ChatGPT web search · 3 searches", "Not available". */
export function describeWebSearch(context: RunContext): string {
  if (!context.webSearch) return 'Not available';
  const parts = ['Enabled'];
  if (context.searchEngines.length > 0) parts.push(context.searchEngines.join(', '));
  if (context.searches > 0) {
    parts.push(`${context.searches} ${context.searches === 1 ? 'search' : 'searches'}`);
  } else {
    parts.push('not used');
  }
  return parts.join(' · ');
}

export const OUTPUT_KIND_LABELS: Record<RunOutputKind, string> = {
  job_search_results: 'Job search',
  application_watch: 'Report',
  calendar_event: 'Calendar',
};

/** Milliseconds a run has taken so far (or took). */
export function elapsed(run: Pick<TaskRun, 'startedAt' | 'finishedAt'>, now: number): number | null {
  if (run.startedAt === null) return null;
  return (run.finishedAt ?? now) - run.startedAt;
}
