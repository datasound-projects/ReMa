import { useCallback, useEffect, useRef, useState, type RefObject } from 'react';

import { useOptionalAnalytics } from '../../app/analytics';
import { useNavigation } from '../../app/navigation';
import { useLinkedRuns } from '../../hooks/useLinkedRuns';
import { useTaskRun, useTaskRuns } from '../../hooks/useTaskRuns';
import { mayListJobs } from '../../lib/analytics';
import {
  describeJobSettings,
  describeSchedule,
  formatDateTime,
  formatDuration,
  formatTime,
  modelName,
} from '../../lib/format';
import { canRunNow } from '../../lib/taskForm';
import {
  describeWebSearch,
  elapsed,
  failureGuidance,
  isActive,
  OUTPUT_KIND_LABELS,
  runStatus,
  runTags,
  runTitle,
} from '../../lib/taskRuns';
import { analyzeTaskResult, listTaskJobRuns, type LinkedRun } from '../../services/analyticsService';
import { toApiError } from '../../services/ipc';
import type { ModelCatalog } from '../../services/providerService';
import {
  cancelTaskRun,
  runTaskNow,
  type RunOutput,
  type RunProgressEvent,
  type ScheduledTask,
  type TaskRun,
  type TaskRunSummary,
} from '../../services/taskService';
import { AnalyzeButton } from '../analytics/AnalyzeButton';
import { Markdown } from '../chat/Markdown';
import { CheckIcon, ClockIcon, CloseIcon } from '../icons';
import { Collapsible } from '../ui/Collapsible';
import { EmptyState, LoadingState } from '../ui/EmptyState';
import { LinkedText } from '../ui/LinkedText';
import { StatusIndicator } from '../ui/StatusIndicator';
import { JobReport } from './JobReport';
import { TaskActions } from './TaskActions';
import { TaskStatusLabel } from './TaskStatus';

interface TaskRunsViewProps {
  task: ScheduledTask;
  catalog: ModelCatalog | null;
  /** The run to show; the newest when not given. */
  runId: number | null;
  onSelectRun: (runId: number) => void;
  onBack: () => void;
  onEdit: () => void;
}

/**
 * "Scheduled / <Task>": every run of a task. The selected run's result in
 * the main area; the run history and the run's progress, context, outputs
 * and task configuration in the panel on the right.
 */
export function TaskRunsView({ task, catalog, runId, onSelectRun, onBack, onEdit }: TaskRunsViewProps) {
  const history = useTaskRuns(task.id);
  const selectedId = runId ?? history.runs[0]?.id ?? null;
  const { run, error: runError } = useTaskRun(selectedId);
  const [error, setError] = useState<string | null>(null);
  const loadLinked = useCallback(() => listTaskJobRuns(task.id), [task.id]);
  const linked = useLinkedRuns(task.kind.type === 'prompt' ? loadLinked : null, `task-runs:${task.id}`);
  const resultRef = useRef<HTMLDivElement>(null);

  const runNow = () => {
    setError(null);
    runTaskNow(task.id)
      .then(onSelectRun)
      .catch((err: unknown) => setError(toApiError(err).message));
  };

  let main;
  if (history.status === 'loading') {
    main = <LoadingState label="Loading runs…" />;
  } else if (history.status === 'error') {
    main = (
      <p className="notice notice--danger" role="alert">
        {history.error}{' '}
        <button type="button" className="link-button" onClick={() => void history.retry()}>
          Retry
        </button>
      </p>
    );
  } else if (history.runs.length === 0) {
    main = (
      <EmptyState
        icon={<ClockIcon />}
        title="No runs yet"
        framed
        compact
        actions={
          canRunNow(task) && (
            <button type="button" className="button button--secondary" onClick={runNow}>
              Run now
            </button>
          )
        }
      >
        This task has not run yet. Its execution history and results will appear here.
      </EmptyState>
    );
  } else if (run) {
    main = (
      <RunView
        run={run}
        catalog={catalog}
        linked={linked.get(run.id)}
        resultRef={resultRef}
        onEdit={onEdit}
        onError={setError}
      />
    );
  } else if (runError) {
    main = (
      <p className="notice notice--danger" role="alert">
        {runError}
      </p>
    );
  } else {
    main = <LoadingState label="Loading the run…" />;
  }

  return (
    <div className="task-runs">
      <div className="task-runs__main">
        <div className="task-runs__inner">
          <header className="task-runs__header">
            <div className="task-runs__title-row">
              <nav className="breadcrumb" aria-label="Breadcrumb">
                <button type="button" className="breadcrumb__link" onClick={onBack}>
                  Scheduled
                </button>
                <span className="breadcrumb__separator" aria-hidden="true">
                  /
                </span>
                <h1 className="breadcrumb__current">{task.name}</h1>
              </nav>
              <div className="page__actions">
                <button
                  type="button"
                  className="button button--secondary"
                  disabled={!canRunNow(task)}
                  onClick={runNow}
                >
                  {task.running ? 'Running…' : 'Run now'}
                </button>
                <TaskActions
                  task={task}
                  onEdit={onEdit}
                  onDeleted={onBack}
                  onError={setError}
                  onRan={onSelectRun}
                />
              </div>
            </div>
            <div className="meta-line">
              <span>{describeSchedule(task.schedule, task.startTime)}</span>
              <span>{modelName(catalog, task.model)}</span>
              {task.nextRunAt !== null && (
                <span>
                  <span className="meta-line__label">Next run</span> {formatDateTime(task.nextRunAt)}
                </span>
              )}
              <TaskStatusLabel task={task} />
            </div>
          </header>
          {error && (
            <p className="form-error" role="alert">
              {error}
            </p>
          )}
          {main}
        </div>
      </div>
      <aside className="task-runs__side" aria-label="Run history">
        <div className="task-runs__side-head">
          <h2 className="task-runs__side-title">Runs</h2>
          {history.runs.length > 0 && (
            <span className="task-runs__side-meta">Newest first</span>
          )}
        </div>
        <RunList
          runs={history.runs}
          selectedId={selectedId}
          hasMore={history.hasMore}
          loadingMore={history.loadingMore}
          onLoadMore={history.loadMore}
          onSelect={onSelectRun}
        />
        {run && (
          <RunInspector
            key={run.id}
            run={run}
            task={task}
            catalog={catalog}
            onShowResult={() => resultRef.current?.scrollIntoView({ block: 'start', behavior: 'smooth' })}
          />
        )}
      </aside>
    </div>
  );
}

/** Re-renders every second while `active` (elapsed time). */
function useNow(active: boolean): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!active) return;
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [active]);
  return now;
}

/** The history: one compact row per run, newest first, loaded as needed. */
function RunList({
  runs,
  selectedId,
  hasMore,
  loadingMore,
  onLoadMore,
  onSelect,
}: {
  runs: TaskRunSummary[];
  selectedId: number | null;
  hasMore: boolean;
  loadingMore: boolean;
  onLoadMore: () => Promise<void>;
  onSelect: (runId: number) => void;
}) {
  const sentinel = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const node = sentinel.current;
    if (!node || !hasMore || typeof IntersectionObserver === 'undefined') return;
    const observer = new IntersectionObserver((entries) => {
      if (entries.some((e) => e.isIntersecting)) void onLoadMore();
    });
    observer.observe(node);
    return () => observer.disconnect();
  }, [hasMore, onLoadMore]);

  if (runs.length === 0) {
    return <p className="run-list__empty">No runs yet</p>;
  }
  return (
    <div className="run-list">
      <ol className="run-list__items">
        {runs.map((run) => {
          const selected = run.id === selectedId;
          const tags = runTags(run);
          const status = runStatus(run);
          return (
            <li key={run.id}>
              <button
                type="button"
                className={selected ? 'run-list__row run-list__row--selected' : 'run-list__row'}
                aria-current={selected ? 'true' : undefined}
                onClick={() => onSelect(run.id)}
              >
                <span className={`run-list__dot run-list__dot--${status.tone}`} aria-hidden="true" />
                <span className="run-list__title">{runTitle(run)}</span>
                {tags.length > 0 && <span className="run-list__tags">{tags.join(' · ')}</span>}
              </button>
            </li>
          );
        })}
      </ol>
      {hasMore && (
        <div ref={sentinel} className="run-list__more">
          <button
            type="button"
            className="button button--ghost button--small"
            disabled={loadingMore}
            onClick={() => void onLoadMore()}
          >
            {loadingMore ? 'Loading…' : 'Load older runs'}
          </button>
        </div>
      )}
    </div>
  );
}

const STAGE_TEXT: Record<RunProgressEvent['status'], string> = {
  pending: 'Not started',
  running: 'In progress',
  completed: 'Done',
  failed: 'Failed',
  skipped: 'Skipped',
};

/** A run's stages as they happened: ✓ done, ● in progress, ○ ahead. */
function RunProgress({ stages }: { stages: RunProgressEvent[] }) {
  if (stages.length === 0) {
    return <p className="run-facts__missing">No activity was recorded for this run.</p>;
  }
  return (
    <ol className="run-progress">
      {stages.map((stage) => (
        <li key={stage.id} className={`run-progress__item run-progress__item--${stage.status}`}>
          <span className="run-progress__mark" aria-hidden="true">
            {stage.status === 'completed' ? (
              <CheckIcon />
            ) : stage.status === 'failed' ? (
              <CloseIcon />
            ) : (
              <span className="run-progress__dot" />
            )}
          </span>
          <span className="sr-only">{STAGE_TEXT[stage.status]}: </span>
          <span className="run-progress__label">{stage.label}</span>
        </li>
      ))}
    </ol>
  );
}

/** The selected run: header, then its result, its progress or its error. */
function RunView({
  run,
  catalog,
  linked,
  resultRef,
  onEdit,
  onError,
}: {
  run: TaskRun;
  catalog: ModelCatalog | null;
  /** The job search Analytics made from this run, if any. */
  linked: LinkedRun | undefined;
  resultRef: RefObject<HTMLDivElement | null>;
  onEdit: () => void;
  onError: (message: string) => void;
}) {
  const { navigate } = useNavigation();
  const active = isActive(run);
  const now = useNow(active);
  const status = runStatus(run);
  // An interrupted run ended when ReMa noticed it, so its length is unknown.
  const interrupted = run.errorCategory === 'interrupted';
  const took = interrupted ? null : elapsed(run, now);
  const guidance = failureGuidance(run.errorCategory);
  // The occurrence is worth showing only when the run started noticeably later.
  const late = run.scheduledFor !== null && run.queuedAt - run.scheduledFor > 60_000;

  return (
    <article className="run-view" aria-labelledby={`run-${run.id}-title`}>
      <header className="run-view__header">
        <h2 id={`run-${run.id}-title`} className="run-view__title">
          {runTitle(run)}
        </h2>
        <div className="meta-line">
          <StatusIndicator tone={status.tone} label={status.label} live={active} />
          <span>{run.trigger === 'manual' ? 'Manual' : 'Scheduled'}</span>
          <span>{modelName(catalog, run.model)}</span>
        </div>
        <dl className="run-view__times">
          {run.startedAt !== null && (
            <div>
              <dt>Started</dt>
              <dd>{formatTime(run.startedAt)}</dd>
            </div>
          )}
          {run.finishedAt !== null && !interrupted && (
            <div>
              <dt>{run.status === 'failed' ? 'Failed' : run.status === 'cancelled' ? 'Ended' : 'Finished'}</dt>
              <dd>{formatTime(run.finishedAt)}</dd>
            </div>
          )}
          {took !== null && (
            <div>
              <dt>{active ? 'Elapsed' : 'Duration'}</dt>
              <dd>{formatDuration(took)}</dd>
            </div>
          )}
          {late && run.scheduledFor !== null && (
            <div>
              <dt>Scheduled for</dt>
              <dd>{formatDateTime(run.scheduledFor)}</dd>
            </div>
          )}
        </dl>
      </header>

      {active ? (
        <section className="run-view__running" aria-label="Progress">
          <RunProgress stages={run.progress} />
          <p className="run-view__note">The result appears here when the run finishes.</p>
          {run.status === 'running' && (
            <div>
              <button
                type="button"
                className="button button--secondary button--small"
                onClick={() => {
                  cancelTaskRun(run.id).catch((err: unknown) => onError(toApiError(err).message));
                }}
              >
                Stop run
              </button>
            </div>
          )}
        </section>
      ) : run.status === 'succeeded' ? (
        <div className="run-view__result" ref={resultRef}>
          {run.report ? <JobReport report={run.report} /> : <Markdown source={run.result ?? ''} />}
          {!run.report && (linked || mayListJobs(run.result ?? '')) && (
            <div className="history__actions">
              <AnalyzeButton run={linked} analyze={() => analyzeTaskResult(run.id)} />
            </div>
          )}
        </div>
      ) : (
        <div className={run.status === 'failed' ? 'run-error' : 'run-error run-error--quiet'} role="alert">
          <p className="run-error__message">
            <LinkedText text={run.error ?? 'The run did not finish.'} />
          </p>
          {guidance && <p className="run-error__hint">{guidance.text}</p>}
          {guidance?.action && (
            <div className="run-error__actions">
              {guidance.action === 'connectors' && (
                <button
                  type="button"
                  className="button button--secondary button--small"
                  onClick={() => navigate({ page: 'settings', focus: 'connectors' })}
                >
                  Open Connectors
                </button>
              )}
              {guidance.action === 'settings' && (
                <button
                  type="button"
                  className="button button--secondary button--small"
                  onClick={() => navigate({ page: 'settings' })}
                >
                  Open Settings
                </button>
              )}
              {guidance.action === 'edit' && (
                <button type="button" className="button button--secondary button--small" onClick={onEdit}>
                  Edit task
                </button>
              )}
            </div>
          )}
        </div>
      )}
    </article>
  );
}

/** Progress, context, outputs and the task as it was, for the selected run. */
function RunInspector({
  run,
  task,
  catalog,
  onShowResult,
}: {
  run: TaskRun;
  task: ScheduledTask;
  catalog: ModelCatalog | null;
  onShowResult: () => void;
}) {
  const analytics = useOptionalAnalytics();
  const { navigate } = useNavigation();
  const open = (output: RunOutput) => {
    const ref = output.reference;
    if (ref.type === 'run_report') onShowResult();
    else if (ref.type === 'job_search') analytics?.openAnalytics({ runIds: [ref.id], tab: 'jobs' });
    else navigate({ page: 'applications', applicationId: ref.id });
  };
  const context = run.context;
  // Runs from before snapshots have no kind; a task's kind never changes.
  const kind = run.kind ?? task.kind;
  const mail = kind.type === 'job_applications' ? kind : null;

  return (
    <div className="run-inspector">
      <Collapsible title="Progress" count={run.progress.length} defaultOpen={isActive(run)}>
        <RunProgress stages={run.progress} />
      </Collapsible>
      <Collapsible title="Context">
        {context ? (
          <dl className="run-facts">
            <div>
              <dt>Profile</dt>
              <dd>{context.profile ? 'Enabled' : mail ? 'Not used' : 'Disabled'}</dd>
            </div>
            <div>
              <dt>Connectors</dt>
              <dd>{context.connectors.length > 0 ? context.connectors.join(', ') : 'None'}</dd>
            </div>
            <div>
              <dt>Web search</dt>
              <dd>{describeWebSearch(context)}</dd>
            </div>
          </dl>
        ) : (
          <p className="run-facts__missing">Not recorded for runs before ReMa kept run details.</p>
        )}
      </Collapsible>
      <Collapsible title="Outputs" count={run.outputs.length}>
        {run.outputs.length === 0 ? (
          <p className="run-facts__missing">No outputs besides the result.</p>
        ) : (
          <ul className="run-outputs">
            {run.outputs.map((output) => (
              <li key={output.id}>
                <button type="button" className="run-outputs__item" onClick={() => open(output)}>
                  <span className="run-outputs__title">{output.title}</span>
                  <span className="run-outputs__kind">{OUTPUT_KIND_LABELS[output.kind]}</span>
                </button>
              </li>
            ))}
          </ul>
        )}
      </Collapsible>
      <Collapsible title="Task configuration">
        <dl className="run-facts">
          {run.taskName !== null && run.taskName !== task.name && (
            <div>
              <dt>Name then</dt>
              <dd>{run.taskName}</dd>
            </div>
          )}
          <div>
            <dt>{mail ? 'Instructions' : 'Prompt'}</dt>
            <dd className="run-facts__prompt">{run.prompt || 'No extra instructions.'}</dd>
          </div>
          {mail && run.kind && (
            <div>
              <dt>Mail</dt>
              <dd>{describeJobSettings(mail)}</dd>
            </div>
          )}
          <div>
            <dt>Model</dt>
            <dd>{modelName(catalog, run.model)}</dd>
          </div>
          {!mail && (
            <div>
              <dt>Profile enabled</dt>
              <dd>{run.useProfile === null ? 'Not recorded' : run.useProfile ? 'Yes' : 'No'}</dd>
            </div>
          )}
          <div>
            <dt>Schedule</dt>
            <dd>
              {run.schedule
                ? `${describeSchedule(run.schedule.schedule, run.schedule.startTime)} (${run.schedule.timezone})`
                : 'Not recorded'}
            </dd>
          </div>
        </dl>
      </Collapsible>
    </div>
  );
}
