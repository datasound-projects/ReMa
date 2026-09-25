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

/** Lookback choices for job-application tasks, in days. */
export const LOOKBACK_PRESETS = [1, 3, 7, 14, 30];
export const MAX_LOOKBACK_DAYS = 90;

/** Editable state of the task dialog. Dates/times are local `YYYY-MM-DD` / `HH:MM`. */
export interface TaskForm {
  name: string;
  type: TaskType;
  /** A preset number of days, or 'custom'. */
  lookback: string;
  customLookback: number;
  syncCalendar: boolean;
  detectConflicts: boolean;
  prompt: string;
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
export function defaultForm(prompt: string, model: ModelRef | null): TaskForm {
  const start = new Date();
  start.setSeconds(0, 0);
  start.setMinutes(Math.ceil((start.getMinutes() + 1) / 15) * 15);
  const startDate = toDateInput(start);
  const end = new Date(start);
  end.setDate(end.getDate() + 7);
  return {
    name: '',
    type: 'prompt',
    lookback: '7',
    customLookback: 60,
    syncCalendar: true,
    detectConflicts: true,
    prompt,
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

export function formFromTask(task: ScheduledTask): TaskForm {
  const form = defaultForm(task.prompt, task.model);
  form.name = task.name;
  if (task.kind.type === 'job_applications') {
    const days = task.kind.lookbackDays;
    form.type = 'job_applications';
    form.lookback = LOOKBACK_PRESETS.includes(days) ? String(days) : 'custom';
    form.customLookback = days;
    form.syncCalendar = task.kind.syncCalendar;
    form.detectConflicts = task.kind.detectConflicts;
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
  const days = form.lookback === 'custom' ? form.customLookback : Number(form.lookback);
  if (!Number.isInteger(days) || days < 1 || days > MAX_LOOKBACK_DAYS) {
    return `Choose a lookback of 1 to ${MAX_LOOKBACK_DAYS} days.`;
  }
  return {
    type: 'job_applications',
    lookbackDays: days,
    syncCalendar: form.syncCalendar,
    detectConflicts: form.syncCalendar && form.detectConflicts,
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
