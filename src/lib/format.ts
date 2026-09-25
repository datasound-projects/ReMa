import type { ModelCatalog, ModelRef } from '../services/providerService';
import type { Schedule, Weekday } from '../services/taskService';

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
