import { useState } from 'react';

import { PlusIcon } from '../components/icons';
import { PageContainer } from '../components/layout/PageContainer';
import { TaskActions } from '../components/tasks/TaskActions';
import { TaskDetail } from '../components/tasks/TaskDetail';
import { TaskDialog } from '../components/tasks/TaskDialog';
import { TaskStatusLabel } from '../components/tasks/TaskStatus';
import { dataOr } from '../hooks/useAsyncData';
import { useModelCatalog } from '../hooks/useModelCatalog';
import { useSystemTimezone, useTasks } from '../hooks/useTasks';
import { describeSchedule, describeTaskKind, formatDateTime, modelName } from '../lib/format';
import type { ScheduledTask } from '../services/taskService';

export function ScheduledTasksPage() {
  const tasks = useTasks();
  const catalog = dataOr(useModelCatalog().state, null);
  const timezone = dataOr(useSystemTimezone().state, 'UTC');
  const [openId, setOpenId] = useState<number | null>(null);
  const [editing, setEditing] = useState<ScheduledTask | 'new' | null>(null);
  const [error, setError] = useState<string | null>(null);

  const list = dataOr(tasks.state, []);
  const open = list.find((t) => t.id === openId);

  const dialog = editing && (
    <TaskDialog
      catalog={catalog}
      timezone={timezone}
      task={editing === 'new' ? undefined : editing}
      initialModel={catalog?.defaultModel ?? null}
      onClose={() => setEditing(null)}
      onSaved={() => setEditing(null)}
    />
  );

  if (open) {
    return (
      <>
        <TaskDetail
          task={open}
          catalog={catalog}
          onBack={() => setOpenId(null)}
          onEdit={() => setEditing(open)}
        />
        {dialog}
      </>
    );
  }

  return (
    <PageContainer
      title="Scheduled Tasks"
      subtitle="Prompts and job-application checks ReMa runs for you on a schedule."
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
      {tasks.state.status === 'error' ? (
        <div className="empty-note">
          {tasks.state.error.message}{' '}
          <button type="button" className="link-button" onClick={tasks.retry}>
            Retry
          </button>
        </div>
      ) : tasks.state.status === 'success' && list.length === 0 ? (
        <p className="empty-note">
          No scheduled tasks yet. Write a prompt in Chat and choose <strong>Schedule</strong>.
        </p>
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
              onClick={() => setOpenId(task.id)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') setOpenId(task.id);
              }}
            >
              <span role="cell" className="task-table__name">
                <span className="task-table__title">{task.name}</span>
                <span className="task-table__prompt">
                  {task.kind.type === 'prompt' ? task.prompt : describeTaskKind(task.kind)}
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
                <TaskActions task={task} onEdit={() => setEditing(task)} onError={setError} />
              </span>
            </div>
          ))}
        </div>
      )}
      {dialog}
    </PageContainer>
  );
}
