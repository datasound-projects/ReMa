import {
  commands,
  type ScheduledTask,
  type TaskInput,
  type TaskRun,
  type TaskRunPage,
} from '../generated/bindings';
import { callBackend } from './ipc';

export type {
  EndCondition,
  ExecutionStatus,
  IntervalUnit,
  Schedule,
  ScheduledTask,
  TaskInput,
  TaskRun,
  TaskRunPage,
  TaskRunSummary,
  RunContext,
  RunErrorCategory,
  RunOutput,
  RunOutputRef,
  RunProgressEvent,
  ScheduleSnapshot,
  StageStatus,
  ExecutionTrigger,
  TaskKind,
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

/** Starts a manual run; resolves with its id. */
export function runTaskNow(id: number): Promise<number> {
  return callBackend(() => commands.runTaskNow(id));
}

/** A task's runs, newest first; `before` is the last run id already shown. */
export function listTaskRuns(taskId: number, before: number | null): Promise<TaskRunPage> {
  return callBackend(() => commands.listTaskRuns(taskId, before));
}

export function getTaskRun(runId: number): Promise<TaskRun> {
  return callBackend(() => commands.getTaskRun(runId));
}

/** Stops a run that is still going. */
export function cancelTaskRun(runId: number): Promise<null> {
  return callBackend(() => commands.cancelTaskRun(runId));
}
