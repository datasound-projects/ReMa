import { useState } from 'react';

import { toApiError } from '../../services/ipc';
import {
  deleteTask,
  runTaskNow,
  setTaskEnabled,
  type ScheduledTask,
} from '../../services/taskService';
import { MoreIcon } from '../icons';
import { Dialog } from '../ui/Dialog';
import { IconButton } from '../ui/IconButton';
import { Menu, type MenuItem } from '../ui/Menu';

interface TaskActionsProps {
  task: ScheduledTask;
  onEdit: () => void;
  onDeleted?: () => void;
  onError: (message: string) => void;
}

/** The "⋯" menu of a task: run now, edit, pause/resume, delete. */
export function TaskActions({ task, onEdit, onDeleted, onError }: TaskActionsProps) {
  const [confirming, setConfirming] = useState(false);

  const attempt = (action: () => Promise<unknown>) => {
    action().catch((error: unknown) => onError(toApiError(error).message));
  };

  const items: MenuItem[] = [
    { label: 'Run now', disabled: task.running, onSelect: () => attempt(() => runTaskNow(task.id)) },
    { label: 'Edit', onSelect: onEdit },
    task.enabled
      ? { label: 'Pause', onSelect: () => attempt(() => setTaskEnabled(task.id, false)) }
      : {
          label: 'Resume',
          disabled: task.status === 'completed',
          onSelect: () => attempt(() => setTaskEnabled(task.id, true)),
        },
    { label: 'Delete', danger: true, onSelect: () => setConfirming(true) },
  ];

  return (
    <>
      <Menu
        items={items}
        trigger={(props) => (
          <IconButton label="Task actions" {...props}>
            <MoreIcon />
          </IconButton>
        )}
      />
      {confirming && (
        <Dialog
          title="Delete task?"
          onClose={() => setConfirming(false)}
          actions={
            <>
              <button type="button" className="button button--secondary" onClick={() => setConfirming(false)}>
                Cancel
              </button>
              <button
                type="button"
                className="button button--danger"
                onClick={() => {
                  setConfirming(false);
                  attempt(async () => {
                    await deleteTask(task.id);
                    onDeleted?.();
                  });
                }}
              >
                Delete
              </button>
            </>
          }
        >
          <p className="dialog__text">
            “{task.name}” and its run history will be removed.
          </p>
        </Dialog>
      )}
    </>
  );
}
