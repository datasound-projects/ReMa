import {
  commands,
  type ScheduledTask,
  type TaskExecution,
  type TaskInput,
} from '../generated/bindings';
import { callBackend } from './ipc';

export type {
  EndCondition,
  ExecutionStatus,
  IntervalUnit,
  Schedule,
  ScheduledTask,
  TaskExecution,
  TaskInput,
  TaskStatus,
  Weekday,
} from '../generated/bindings';

export function listTasks(): Promise<ScheduledTask[]> {
  return callBackend(() => commands.listTasks());
}

export function createTask(input: TaskInput): Promise<ScheduledTask> {
  return callBackend(() => commands.createTask(input));
}

export function updateTask(id: number, input: TaskInput): Promise<ScheduledTask> {
  return callBackend(() => commands.updateTask(id, input));
}

export function setTaskEnabled(id: number, enabled: boolean): Promise<ScheduledTask> {
  return callBackend(() => commands.setTaskEnabled(id, enabled));
}

export function deleteTask(id: number): Promise<null> {
  return callBackend(() => commands.deleteTask(id));
}

export function runTaskNow(id: number): Promise<null> {
  return callBackend(() => commands.runTaskNow(id));
}

export function listTaskExecutions(taskId: number): Promise<TaskExecution[]> {
  return callBackend(() => commands.listTaskExecutions(taskId));
}
