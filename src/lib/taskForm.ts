import type { ModelRef } from '../services/providerService';
import type {
  IntervalUnit,
  Schedule,
  ScheduledTask,
  TaskInput,
  TaskKind,
  Weekday,
} from '../services/taskService';
import { isWorkweek } from './format';

export type TaskType = TaskKind['type'];
export type RepeatKind = 'once' | 'daily' | 'weekdays' | 'weekly' | 'days' | 'interval';
export type EndKind = 'never' | 'on_date' | 'after_runs';

/** What the built-in Job Mail & Interview Sync does, in one sentence. */
export const JOB_MAIL_SYNC_DESCRIPTION =
  'Tracks job-application emails, updates Applications, detects actions and confirmed interviews, and syncs ' +
  'confirmed interviews with your calendar.';

/** "Initial lookback: last N days" for Job Mail & Interview Sync. */
export const DEFAULT_LOOKBACK_DAYS = 30;
export const MAX_LOOKBACK_DAYS = 365;

/**
 * A mail task left from before Job Mail & Interview Sync existed. It stays
 * paused and never runs: only the built-in task reads mail.
 */
export function isOldMailTask(task: ScheduledTask): boolean {
  return task.kind.type === 'job_applications' && task.builtin !== 'job_mail_sync';
}

/**
 * "Run now" is offered unless the task is running, it is Job Mail &
 * Interview Sync while turned off (mail is read only while it is on), or it
 * is an old mail task.
 */
export function canRunNow(task: ScheduledTask): boolean {
  if (task.running || isOldMailTask(task)) return false;
  return task.builtin !== 'job_mail_sync' || task.enabled;
}

/** Editable state of the task dialog. Dates/times are local `YYYY-MM-DD` / `HH:MM`. */
export interface TaskForm {
  name: string;
  type: TaskType;
  /** Job Mail & Interview Sync: the first run reads the last N days. */
  lookbackDays: number;
  syncCalendar: boolean;
  prompt: string;
  /** Prompt tasks: give the model the user's Profile. */
  useProfile: boolean;
  model: ModelRef | null;
  startDate: string;
  startTime: string;
  repeat: RepeatKind;
  everyDays: number;
  intervalEvery: number;
  intervalUnit: IntervalUnit;
  weekdays: Weekday[];
  endKind: EndKind;
  endDate: string;
  endCount: number;
}

const WEEKDAY_BY_INDEX: Weekday[] = ['sun', 'mon', 'tue', 'wed', 'thu', 'fri', 'sat'];

const pad = (n: number) => String(n).padStart(2, '0');

export function toDateInput(date: Date): string {
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
}

function weekdayOf(dateInput: string): Weekday {
  const [y, m, d] = dateInput.split('-').map(Number);
  return WEEKDAY_BY_INDEX[new Date(y ?? 0, (m ?? 1) - 1, d ?? 1).getDay()] ?? 'mon';
}

/** A new task starting at the next quarter hour. */
export function defaultForm(prompt: string, model: ModelRef | null, useProfile = false): TaskForm {
  const start = new Date();
  start.setSeconds(0, 0);
  start.setMinutes(Math.ceil((start.getMinutes() + 1) / 15) * 15);
  const startDate = toDateInput(start);
  const end = new Date(start);
  end.setDate(end.getDate() + 7);
  return {
    name: '',
    type: 'prompt',
    lookbackDays: DEFAULT_LOOKBACK_DAYS,
    syncCalendar: true,
    prompt,
    useProfile,
    model,
    startDate,
    startTime: `${pad(start.getHours())}:${pad(start.getMinutes())}`,
    repeat: 'daily',
    everyDays: 2,
    intervalEvery: 4,
    intervalUnit: 'hours',
    weekdays: [weekdayOf(startDate)],
    endKind: 'never',
    endDate: toDateInput(end),
    endCount: 10,
  };
}

/** Setting up Job Mail & Interview Sync: hourly from now, last 30 days first. */
export function jobMailSyncForm(model: ModelRef | null): TaskForm {
  return {
    ...defaultForm('', model),
    type: 'job_applications',
    repeat: 'interval',
    intervalEvery: 1,
    intervalUnit: 'hours',
  };
}

export function formFromTask(task: ScheduledTask): TaskForm {
  const form = defaultForm(task.prompt, task.model, task.useProfile);
  form.name = task.name;
  if (task.kind.type === 'job_applications') {
    form.type = 'job_applications';
    form.lookbackDays = task.kind.lookbackDays;
    form.syncCalendar = task.kind.syncCalendar;
  }
  form.startDate = task.startDate;
  form.startTime = task.startTime;
  const schedule = task.schedule;
  switch (schedule.kind) {
    case 'once':
      form.repeat = 'once';
      break;
    case 'daily':
      form.repeat = schedule.every === 1 ? 'daily' : 'days';
      form.everyDays = schedule.every;
      break;
    case 'weekly':
      form.repeat = isWorkweek(schedule.days) ? 'weekdays' : 'weekly';
      form.weekdays = schedule.days;
      break;
    case 'interval':
      form.repeat = 'interval';
      form.intervalEvery = schedule.every;
      form.intervalUnit = schedule.unit;
      break;
  }
  switch (task.end.kind) {
    case 'on_date':
      form.endKind = 'on_date';
      form.endDate = task.end.date;
      break;
    case 'after_runs':
      form.endKind = 'after_runs';
      form.endCount = task.end.count;
      break;
    case 'never':
      form.endKind = 'never';
  }
  return form;
}

function toKind(form: TaskForm): TaskKind | string {
  if (form.type === 'prompt') return { type: 'prompt' };
  const days = form.lookbackDays;
  if (!Number.isInteger(days) || days < 1 || days > MAX_LOOKBACK_DAYS) {
    return `Choose a lookback of 1 to ${MAX_LOOKBACK_DAYS} days.`;
  }
  return {
    type: 'job_applications',
    lookbackDays: days,
    syncCalendar: form.syncCalendar,
  };
}

function toSchedule(form: TaskForm): Schedule {
  switch (form.repeat) {
    case 'once':
      return { kind: 'once' };
    case 'daily':
      return { kind: 'daily', every: 1 };
    case 'days':
      return { kind: 'daily', every: form.everyDays };
    case 'weekdays':
      return { kind: 'weekly', days: ['mon', 'tue', 'wed', 'thu', 'fri'] };
    case 'weekly':
      return { kind: 'weekly', days: form.weekdays };
    case 'interval':
      return { kind: 'interval', every: form.intervalEvery, unit: form.intervalUnit };
  }
}

/**
 * Builds the command input. Quick checks here give instant feedback; the
 * backend validates everything again.
 */
export function formToInput(form: TaskForm, timezone: string): TaskInput | string {
  if (form.type === 'prompt' && !form.prompt.trim()) return 'Enter a prompt for the task.';
  if (!form.model) return 'Choose a model.';
  const kind = toKind(form);
  if (typeof kind === 'string') return kind;
  if (!form.startDate || !form.startTime) return 'Choose when the task starts.';
  if (form.repeat === 'interval') {
    const minutes = form.intervalEvery * (form.intervalUnit === 'hours' ? 60 : 1);
    if (!Number.isFinite(minutes) || minutes < 15) return 'Tasks can run at most once every 15 minutes.';
  }
  if (form.repeat === 'weekly' && form.weekdays.length === 0) return 'Choose at least one weekday.';
  return {
    name: form.name.trim(),
    kind,
    prompt: form.prompt.trim(),
    useProfile: form.type === 'prompt' && form.useProfile,
    model: form.model,
    timezone,
    startDate: form.startDate,
    startTime: form.startTime,
    schedule: toSchedule(form),
    end:
      form.repeat === 'once' || form.endKind === 'never'
        ? { kind: 'never' }
        : form.endKind === 'on_date'
          ? { kind: 'on_date', date: form.endDate }
          : { kind: 'after_runs', count: form.endCount },
  };
}
