import { useState } from 'react';

import { useNavigation } from '../app/navigation';
import { ClockIcon, MailCalendarIcon, PlusIcon } from '../components/icons';
import { PageContainer } from '../components/layout/PageContainer';
import { TaskActions } from '../components/tasks/TaskActions';
import { TaskDialog } from '../components/tasks/TaskDialog';
import { TaskRunsView } from '../components/tasks/TaskRunsView';
import { TaskStatusLabel } from '../components/tasks/TaskStatus';
import { EmptyState, LoadingState } from '../components/ui/EmptyState';
import { Switch } from '../components/ui/Switch';
import { useAction } from '../hooks/useAction';
import { dataOr } from '../hooks/useAsyncData';
import { useModelCatalog } from '../hooks/useModelCatalog';
import { useSystemTimezone, useTasks } from '../hooks/useTasks';
import { describeJobSettings, describeSchedule, describeTaskKind, formatDateTime, modelName } from '../lib/format';
import { canRunNow, isOldMailTask, JOB_MAIL_SYNC_DESCRIPTION } from '../lib/taskForm';
import type { ModelCatalog } from '../services/providerService';
import { runTaskNow, setTaskEnabled, type ScheduledTask } from '../services/taskService';

type Editing = ScheduledTask | 'new' | 'job_mail_sync';

interface ScheduledTasksPageProps {
  /** The task whose run history is open. */
  taskId?: number | null;
  /** The run shown in it (the newest when absent). */
  runId?: number | null;
}

export function ScheduledTasksPage({ taskId = null, runId = null }: ScheduledTasksPageProps) {
  const tasks = useTasks();
  const { navigate } = useNavigation();
  const catalog = dataOr(useModelCatalog().state, null);
  const timezone = dataOr(useSystemTimezone().state, 'UTC');
  const [editing, setEditing] = useState<Editing | null>(null);
  const [error, setError] = useState<string | null>(null);

  const all = dataOr(tasks.state, []);
  const jobMailSync = all.find((t) => t.builtin === 'job_mail_sync') ?? null;
  const list = all.filter((t) => t.builtin === null);
  const open = all.find((t) => t.id === taskId);
  const openTask = (id: number, run: number | null = null) =>
    navigate({ page: 'tasks', taskId: id, runId: run });

  const dialog = editing && (
    <TaskDialog
      catalog={catalog}
      timezone={timezone}
      task={typeof editing === 'string' ? undefined : editing}
      jobMailSync={editing === 'job_mail_sync'}
      initialModel={catalog?.defaultModel ?? null}
      onClose={() => setEditing(null)}
      onSaved={() => setEditing(null)}
    />
  );

  if (open) {
    return (
      <>
        <TaskRunsView
          task={open}
          catalog={catalog}
          runId={runId}
          onSelectRun={(id) => openTask(open.id, id)}
          onBack={() => navigate({ page: 'tasks' })}
          onEdit={() => setEditing(open)}
        />
        {dialog}
      </>
    );
  }

  return (
    <PageContainer
      title="Scheduled Tasks"
      subtitle="ReMa's built-in automation and your own prompts, run on a schedule."
      actions={
        <button type="button" className="button button--secondary" onClick={() => setEditing('new')}>
          <PlusIcon className="button__icon" />
          New task
        </button>
      }
    >
      {error && (
        <p className="form-error" role="alert">
          {error}
        </p>
      )}
      {tasks.state.status === 'success' && (
        <section className="section" aria-labelledby="builtin-heading">
          <h2 id="builtin-heading" className="section__title">
            Built-in
          </h2>
          <JobMailSyncCard
            task={jobMailSync}
            catalog={catalog}
            onSetUp={() => setEditing('job_mail_sync')}
            onOpen={(id, run) => openTask(id, run)}
            onEdit={(task) => setEditing(task)}
            onError={setError}
          />
        </section>
      )}
      {tasks.state.status === 'success' && (
        <h2 className="section__title tasks__own-heading">Your tasks</h2>
      )}
      {tasks.state.status === 'error' ? (
        <p className="notice notice--danger" role="alert">
          {tasks.state.error.message}{' '}
          <button type="button" className="link-button" onClick={tasks.retry}>
            Retry
          </button>
        </p>
      ) : tasks.state.status === 'loading' ? (
        <LoadingState label="Loading tasks…" />
      ) : list.length === 0 ? (
        <EmptyState
          icon={<ClockIcon />}
          title="No tasks of your own yet"
          framed
          compact
          actions={
            <button type="button" className="button button--secondary" onClick={() => setEditing('new')}>
              <PlusIcon className="button__icon" />
              New task
            </button>
          }
        >
          Let ReMa repeat a prompt on a schedule. You can also write a prompt in Chat and choose Schedule.
        </EmptyState>
      ) : (
        <div className="task-table" role="table" aria-label="Scheduled tasks">
          <div className="task-table__head" role="row">
            <span role="columnheader">Task</span>
            <span role="columnheader">Schedule</span>
            <span role="columnheader">Model</span>
            <span role="columnheader">Next run</span>
            <span role="columnheader">Status</span>
            <span role="columnheader" aria-label="Actions" />
          </div>
          {list.map((task) => (
            <div
              key={task.id}
              className="task-table__row"
              role="row"
              tabIndex={0}
              onClick={() => openTask(task.id)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') openTask(task.id);
              }}
            >
              <span role="cell" className="task-table__name">
                <span className="task-table__title">{task.name}</span>
                <span className="task-table__prompt">
                  {isOldMailTask(task)
                    ? 'No longer runs: only Job Mail & Interview Sync reads mail. You can delete this task.'
                    : task.kind.type === 'prompt'
                      ? task.prompt
                      : describeTaskKind(task.kind)}
                </span>
              </span>
              <span role="cell">{describeSchedule(task.schedule, task.startTime)}</span>
              <span role="cell" className="task-table__muted">
                {modelName(catalog, task.model)}
              </span>
              <span role="cell" className="task-table__muted">
                {task.nextRunAt !== null ? formatDateTime(task.nextRunAt) : '—'}
              </span>
              <span role="cell">
                <TaskStatusLabel task={task} />
              </span>
              <span
                role="cell"
                className="task-table__actions"
                // Menu and dialogs inside the row must not open the task.
                onClick={(e) => e.stopPropagation()}
                onKeyDown={(e) => e.stopPropagation()}
              >
                <TaskActions
                  task={task}
                  onEdit={() => setEditing(task)}
                  onError={setError}
                  onRan={(run) => openTask(task.id, run)}
                />
              </span>
            </div>
          ))}
        </div>
      )}
      {dialog}
    </PageContainer>
  );
}

/**
 * The built-in Job Mail & Interview Sync. Mail is read only after the user
 * sets it up (which turns it on) and while it stays on.
 */
function JobMailSyncCard({
  task,
  catalog,
  onSetUp,
  onOpen,
  onEdit,
  onError,
}: {
  task: ScheduledTask | null;
  catalog: ModelCatalog | null;
  onSetUp: () => void;
  /** Opens the task's runs (at `run`, or the newest). */
  onOpen: (id: number, run?: number | null) => void;
  onEdit: (task: ScheduledTask) => void;
  onError: (message: string) => void;
}) {
  const action = useAction();
  return (
    <article className="builtin-card" aria-label="Job Mail & Interview Sync">
      <span className="builtin-card__icon">
        <MailCalendarIcon />
      </span>
      <div className="builtin-card__body">
        <div className="builtin-card__head">
          <h3 className="builtin-card__title">Job Mail &amp; Interview Sync</h3>
          {task && <TaskStatusLabel task={task} />}
        </div>
        <p className="builtin-card__description">{JOB_MAIL_SYNC_DESCRIPTION}</p>
        {task ? (
          <p className="builtin-card__meta">
            {task.kind.type === 'job_applications' && `${describeJobSettings(task.kind)} · `}
            {describeSchedule(task.schedule, task.startTime)} ·{' '}
            {modelName(catalog, task.model)}
            {task.nextRunAt !== null && ` · next run ${formatDateTime(task.nextRunAt)}`}
          </p>
        ) : (
          <p className="builtin-card__meta">Off. ReMa does not read your mail until you set this up.</p>
        )}
        {action.error && <p className="form-error">{action.error}</p>}
      </div>
      <div className="builtin-card__actions">
        {task ? (
          <>
            <Switch
              checked={task.enabled}
              disabled={action.busy || task.status === 'completed'}
              aria-label="Job Mail & Interview Sync"
              onChange={(on) => void action.run(() => setTaskEnabled(task.id, on))}
            />
            <button
              type="button"
              className="button button--secondary button--small"
              disabled={!canRunNow(task)}
              onClick={() => void action.run(() => runTaskNow(task.id).then((run) => onOpen(task.id, run)))}
            >
              {task.running ? 'Running…' : 'Run now'}
            </button>
            <button type="button" className="button button--ghost button--small" onClick={() => onOpen(task.id)}>
              History
            </button>
            <TaskActions
              task={task}
              onEdit={() => onEdit(task)}
              onError={onError}
              onRan={(run) => onOpen(task.id, run)}
            />
          </>
        ) : (
          <button type="button" className="button button--primary button--small" onClick={onSetUp}>
            Set up
          </button>
        )}
      </div>
    </article>
  );
}
