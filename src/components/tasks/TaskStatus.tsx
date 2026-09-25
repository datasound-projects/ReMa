import type { ScheduledTask } from '../../services/taskService';
import { StatusIndicator } from '../ui/StatusIndicator';

/** Lifecycle of a task as a status dot and label. */
export function TaskStatusLabel({ task }: { task: ScheduledTask }) {
  if (task.running) return <StatusIndicator tone="pending" label="Running" />;
  switch (task.status) {
    case 'active':
      return <StatusIndicator tone="ready" label="Active" />;
    case 'paused':
      return <StatusIndicator tone="idle" label="Paused" />;
    case 'completed':
      return <StatusIndicator tone="idle" label="Completed" />;
  }
}
