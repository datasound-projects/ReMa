import { useState } from 'react';

import { dataOr } from '../../hooks/useAsyncData';
import { useTaskExecutions } from '../../hooks/useTasks';
import {
  describeSchedule,
  describeTaskKind,
  formatDateTime,
  formatDuration,
  modelName,
} from '../../lib/format';
import { toApiError } from '../../services/ipc';
import type { ModelCatalog } from '../../services/providerService';
import { runTaskNow, type ScheduledTask, type TaskExecution } from '../../services/taskService';
import { Markdown } from '../chat/Markdown';
import { ChevronLeftIcon, ChevronRightIcon } from '../icons';
import { JobReport } from './JobReport';
import { StatusIndicator } from '../ui/StatusIndicator';
import { TaskActions } from './TaskActions';
import { TaskStatusLabel } from './TaskStatus';

interface TaskDetailProps {
  task: ScheduledTask;
  catalog: ModelCatalog | null;
  onBack: () => void;
  onEdit: () => void;
}

/** One task: its definition and run history with results. */
export function TaskDetail({ task, catalog, onBack, onEdit }: TaskDetailProps) {
  const history = useTaskExecutions(task.id);
  const executions = dataOr(history.state, []);
  const [openId, setOpenId] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);

  return (
    <div className="page">
      <div className="page__inner">
        <button type="button" className="back-link" onClick={onBack}>
          <ChevronLeftIcon />
          Scheduled Tasks
        </button>

        <header className="page__header">
          <div className="page__heading">
            <h1 className="page__title">{task.name}</h1>
            <p className="page__subtitle task-meta">
              <span>{describeSchedule(task.schedule, task.startTime)}</span>
              <span>{modelName(catalog, task.model)}</span>
              {task.nextRunAt !== null && <span>Next {formatDateTime(task.nextRunAt)}</span>}
              <TaskStatusLabel task={task} />
            </p>
          </div>
          <div className="page__actions">
            <button
              type="button"
              className="button button--secondary"
              disabled={task.running}
              onClick={() =>
                runTaskNow(task.id).catch((e: unknown) => setError(toApiError(e).message))
              }
            >
              Run now
            </button>
            <TaskActions task={task} onEdit={onEdit} onDeleted={onBack} onError={setError} />
          </div>
        </header>

        {error && (
          <p className="form-error" role="alert">
            {error}
          </p>
        )}

        {task.kind.type === 'job_applications' ? (
          <section>
            <h2 className="section-title">Job applications</h2>
            <p className="prompt-box">
              {describeTaskKind(task.kind)}
              {task.kind.detectConflicts && ' · Conflicts reported'}
              {task.prompt && (
                <>
                  <br />
                  {task.prompt}
                </>
              )}
            </p>
          </section>
        ) : (
          <section>
            <h2 className="section-title">Prompt</h2>
            <p className="prompt-box">{task.prompt}</p>
          </section>
        )}

        <section>
          <h2 className="section-title">History</h2>
          {executions.length === 0 ? (
            <p className="empty-note">No runs yet.</p>
          ) : (
            <ul className="history">
              {executions.map((execution) => (
                <HistoryRow
                  key={execution.id}
                  execution={execution}
                  open={openId === execution.id}
                  onToggle={() => setOpenId(openId === execution.id ? null : execution.id)}
                />
              ))}
            </ul>
          )}
        </section>
      </div>
    </div>
  );
}

function HistoryRow({
  execution,
  open,
  onToggle,
}: {
  execution: TaskExecution;
  open: boolean;
  onToggle: () => void;
}) {
  const status =
    execution.status === 'running' ? (
      <StatusIndicator tone="pending" label="Running" />
    ) : execution.status === 'succeeded' ? (
      <StatusIndicator tone="ready" label="Success" />
    ) : (
      <StatusIndicator tone="error" label="Failed" />
    );
  const finished = execution.status !== 'running';
  return (
    <li className={open ? 'history__item history__item--open' : 'history__item'}>
      <button
        type="button"
        className="history__row"
        aria-expanded={open}
        disabled={!finished}
        onClick={onToggle}
      >
        <span className="history__time">{formatDateTime(execution.startedAt)}</span>
        {status}
        <span className="history__meta">
          {execution.trigger === 'manual' && 'Run manually · '}
          {execution.finishedAt !== null && formatDuration(execution.finishedAt - execution.startedAt)}
        </span>
        {finished && <ChevronRightIcon className="history__chevron" />}
      </button>
      {open && (
        <div className="history__result">
          {execution.error ? (
            <p className="message__error">{execution.error}</p>
          ) : execution.report ? (
            <JobReport report={execution.report} />
          ) : (
            <Markdown source={execution.result ?? ''} />
          )}
        </div>
      )}
    </li>
  );
}
