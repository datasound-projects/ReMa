import { backendEvents } from '../services/events';
import { listTasks } from '../services/taskService';
import { getSystemTimezone } from '../services/systemService';
import { useAsyncData } from './useAsyncData';
import { useBackendEvent } from './useBackendEvent';

/** All scheduled tasks, updated as the scheduler runs them. */
export function useTasks() {
  const data = useAsyncData(listTasks);
  useBackendEvent(backendEvents.tasksChanged, data.refresh);
  return data;
}

/** The computer's IANA timezone (default for new tasks). */
export function useSystemTimezone() {
  return useAsyncData(getSystemTimezone);
}
