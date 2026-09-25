import type { ApplicationStatus } from '../generated/bindings';
import type { ModelCatalog, ModelRef } from '../services/providerService';
import type { Schedule, TaskKind, Weekday } from '../services/taskService';

const time = new Intl.DateTimeFormat(undefined, { hour: '2-digit', minute: '2-digit' });
const dayMonth = new Intl.DateTimeFormat(undefined, { day: 'numeric', month: 'short' });
const dayMonthYear = new Intl.DateTimeFormat(undefined, {
  day: 'numeric',
  month: 'short',
  year: 'numeric',
});

function startOfDay(date: Date): number {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime();
}

/** "Today 18:00", "Tomorrow 08:00", "27 Sep 14:00", "3 Jan 2027 09:00". */
export function formatDateTime(ms: number, now = new Date()): string {
  const date = new Date(ms);
  const days = Math.round((startOfDay(date) - startOfDay(now)) / 86_400_000);
  const clock = time.format(date);
  if (days === 0) return `Today ${clock}`;
  if (days === 1) return `Tomorrow ${clock}`;
  if (days === -1) return `Yesterday ${clock}`;
  const sameYear = date.getFullYear() === now.getFullYear();
  return `${(sameYear ? dayMonth : dayMonthYear).format(date)} ${clock}`;
}

/** A local `HH:MM` time in the user's clock format ("08:00" or "8:00 AM"). */
export function formatClock(hhmm: string): string {
  const [hours = 0, minutes = 0] = hhmm.split(':').map(Number);
  const date = new Date();
  date.setHours(hours, minutes, 0, 0);
  return time.format(date);
}

/** "12s", "3m 5s". */
export function formatDuration(ms: number): string {
  const seconds = Math.max(0, Math.round(ms / 1000));
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  return `${minutes}m ${seconds % 60}s`;
}

export const WEEKDAYS: { id: Weekday; short: string; letter: string }[] = [
  { id: 'mon', short: 'Mon', letter: 'M' },
  { id: 'tue', short: 'Tue', letter: 'T' },
  { id: 'wed', short: 'Wed', letter: 'W' },
  { id: 'thu', short: 'Thu', letter: 'T' },
  { id: 'fri', short: 'Fri', letter: 'F' },
  { id: 'sat', short: 'Sat', letter: 'S' },
  { id: 'sun', short: 'Sun', letter: 'S' },
];

const WORKWEEK: Weekday[] = ['mon', 'tue', 'wed', 'thu', 'fri'];

export function isWorkweek(days: Weekday[]): boolean {
  return days.length === 5 && WORKWEEK.every((d) => days.includes(d));
}

/** "Once", "Daily 08:00", "Every 4 hours", "Weekdays 09:00", "Mon, Thu 08:00". */
export function describeSchedule(schedule: Schedule, startTime: string): string {
  const at = formatClock(startTime);
  switch (schedule.kind) {
    case 'once':
      return 'Once';
    case 'interval': {
      const unit = schedule.unit === 'hours' ? 'hour' : 'minute';
      return schedule.every === 1 ? `Every ${unit}` : `Every ${schedule.every} ${unit}s`;
    }
    case 'daily':
      return schedule.every === 1 ? `Daily ${at}` : `Every ${schedule.every} days ${at}`;
    case 'weekly': {
      if (schedule.days.length === 7) return `Daily ${at}`;
      if (isWorkweek(schedule.days)) return `Weekdays ${at}`;
      const names = WEEKDAYS.filter((d) => schedule.days.includes(d.id)).map((d) => d.short);
      return `${names.join(', ')} ${at}`;
    }
  }
}

/** Display name of a model, falling back to its id when not in the catalog. */
export function modelName(catalog: ModelCatalog | null, model: ModelRef): string {
  const option = catalog?.models.find(
    (o) => o.model.providerId === model.providerId && o.model.modelId === model.modelId,
  );
  return option?.displayName ?? model.modelId;
}

export const APPLICATION_STATUS_LABELS: Record<ApplicationStatus, string> = {
  confirmed: 'Confirmed',
  in_process: 'Application in Process',
  needs_action: 'Needs Your Action',
  upcoming_interview: 'Upcoming Interview',
  rejected: 'Rejected',
};

/** "Job applications · 7 days · Calendar sync". */
export function describeTaskKind(kind: TaskKind): string {
  if (kind.type === 'prompt') return 'Prompt';
  const days = kind.lookbackDays === 1 ? '1 day' : `${kind.lookbackDays} days`;
  return ['Job applications', days, kind.syncCalendar && 'Calendar sync'].filter(Boolean).join(' · ');
}

/** "14:30–15:30" in `timezone` (local time without one). */
export function formatTimeRange(start: number, end: number, timezone: string | null): string {
  const options: Intl.DateTimeFormatOptions = { hour: '2-digit', minute: '2-digit' };
  let format = time;
  try {
    if (timezone) format = new Intl.DateTimeFormat(undefined, { ...options, timeZone: timezone });
  } catch {
    // Unknown zone: fall back to local time.
  }
  return `${format.format(new Date(start))}–${format.format(new Date(end))}`;
}

/** A time shown in the timezone it was stated in, e.g. "Thu, 24 Sep, 14:00 (Europe/Vienna)". */
export function formatInTimezone(ms: number, timezone: string | null): string {
  if (!timezone) return formatDateTime(ms);
  try {
    const text = new Intl.DateTimeFormat(undefined, {
      timeZone: timezone,
      weekday: 'short',
      day: 'numeric',
      month: 'short',
      hour: '2-digit',
      minute: '2-digit',
    }).format(new Date(ms));
    return `${text} (${timezone})`;
  } catch {
    return formatDateTime(ms);
  }
}
