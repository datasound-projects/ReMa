import { useCallback, useEffect, useRef, useState } from 'react';

import { useCoverBrowser } from '../../app/browser';
import { useNavigation } from '../../app/navigation';
import { dataOr, useAsyncData } from '../../hooks/useAsyncData';
import { useBackendEvent } from '../../hooks/useBackendEvent';
import { formatTimeRange } from '../../lib/format';
import { getCalendar, type CalendarEntry, type CalendarView } from '../../services/applicationService';
import type { ProviderId } from '../../services/connectorService';
import { backendEvents } from '../../services/events';
import { openExternalUrl } from '../../services/systemService';
import { CalendarIcon, ChevronLeftIcon, ChevronRightIcon, ExternalIcon } from '../icons';
import { IconButton } from '../ui/IconButton';

/** Days shown at once. */
const SPAN = 7;

/** Calendars come only from Google and Microsoft accounts. */
const PROVIDER_NAMES: Partial<Record<ProviderId, string>> = { google: 'Google', microsoft: 'Outlook' };

function startOfDay(ms: number): number {
  const date = new Date(ms);
  return new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime();
}

/** The same local day `days` later (DST-safe). */
function addDays(ms: number, days: number): number {
  const date = new Date(ms);
  return new Date(date.getFullYear(), date.getMonth(), date.getDate() + days).getTime();
}

const dayHeading = new Intl.DateTimeFormat(undefined, { weekday: 'short', day: 'numeric', month: 'short' });
const rangeDay = new Intl.DateTimeFormat(undefined, { day: 'numeric', month: 'short' });

/**
 * The calendar button in the title bar: a quick view of the connected
 * Google and Outlook calendars inside ReMa, with confirmed interviews
 * marked and their meeting links one click away.
 */
export function CalendarButton() {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onPointer = (event: PointerEvent) => {
      if (ref.current && !ref.current.contains(event.target as Node)) setOpen(false);
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setOpen(false);
    };
    window.addEventListener('pointerdown', onPointer);
    window.addEventListener('keydown', onKey);
    return () => {
      window.removeEventListener('pointerdown', onPointer);
      window.removeEventListener('keydown', onKey);
    };
  }, [open]);

  return (
    <div className="calendar-popover" ref={ref}>
      <IconButton
        label="Calendar"
        aria-expanded={open}
        aria-haspopup="dialog"
        className="icon-button--small"
        onClick={() => setOpen(!open)}
      >
        <CalendarIcon />
      </IconButton>
      {open && <CalendarPanel onClose={() => setOpen(false)} />}
    </div>
  );
}

export function CalendarPanel({ onClose }: { onClose: () => void }) {
  // Web pages are drawn natively above ReMa; hide the page meanwhile.
  useCoverBrowser();
  // Today when the panel opened (render stays pure).
  const [today] = useState(() => startOfDay(Date.now()));
  const [from, setFrom] = useState(today);
  const to = addDays(from, SPAN);

  return (
    <div className="calendar-panel" role="dialog" aria-label="Calendar">
      <div className="calendar-panel__head">
        <span className="calendar-panel__title">Calendar</span>
        <div className="calendar-panel__nav">
          <IconButton label="Previous week" className="icon-button--small" onClick={() => setFrom(addDays(from, -SPAN))}>
            <ChevronLeftIcon />
          </IconButton>
          <button
            type="button"
            className="button button--ghost button--small"
            disabled={from === today}
            onClick={() => setFrom(today)}
          >
            Today
          </button>
          <IconButton label="Next week" className="icon-button--small" onClick={() => setFrom(addDays(from, SPAN))}>
            <ChevronRightIcon />
          </IconButton>
        </div>
      </div>
      <p className="calendar-panel__range" aria-live="polite">
        {rangeDay.format(from)} – {rangeDay.format(addDays(from, SPAN - 1))}
      </p>
      {/* A new range loads afresh. */}
      <CalendarWeek key={from} from={from} to={to} today={today} onClose={onClose} />
    </div>
  );
}

function CalendarWeek({
  from,
  to,
  today,
  onClose,
}: {
  from: number;
  to: number;
  today: number;
  onClose: () => void;
}) {
  const { navigate } = useNavigation();
  const load = useCallback(() => getCalendar(from, to), [from, to]);
  const calendar = useAsyncData(load);
  useBackendEvent(backendEvents.applicationsChanged, calendar.refresh);
  useBackendEvent(backendEvents.connectorsChanged, calendar.refresh);
  const view = dataOr(calendar.state, null);

  if (calendar.state.status === 'loading') {
    return <p className="calendar-panel__empty">Loading your calendar…</p>;
  }
  if (calendar.state.status === 'error') {
    return (
      <p className="calendar-panel__empty" role="alert">
        {calendar.state.error.message}{' '}
        <button type="button" className="link-button" onClick={calendar.retry}>
          Try again
        </button>
      </p>
    );
  }
  if (!view) return null;
  if (view.calendars.length === 0) {
    return (
      <div className="calendar-panel__empty">
        <p className="calendar-panel__empty-title">No calendar connected.</p>
        <p>Connect Google Calendar or Outlook Calendar in Settings → Connectors.</p>
        <button
          type="button"
          className="button button--secondary button--small"
          onClick={() => {
            onClose();
            navigate({ page: 'settings', focus: 'connectors' });
          }}
        >
          Open Connectors
        </button>
      </div>
    );
  }
  return (
    <div className="calendar-panel__body">
      {view.problems.map((problem) => (
        <p key={problem} className="calendar-panel__problem" role="alert">
          {problem}
        </p>
      ))}
      <Days
        view={view}
        from={from}
        today={today}
        onOpenApplication={(id) => {
          onClose();
          navigate({ page: 'applications', applicationId: id });
        }}
      />
    </div>
  );
}

function Days({
  view,
  from,
  today,
  onOpenApplication,
}: {
  view: CalendarView;
  from: number;
  today: number;
  onOpenApplication: (id: number) => void;
}) {
  const showProvider = new Set(view.calendars.map((c) => c.provider)).size > 1;
  const days = Array.from({ length: SPAN }, (_, i) => addDays(from, i));
  const byDay = days.map((day) => ({
    day,
    entries: view.entries.filter((e) => e.startAt < addDays(day, 1) && e.endAt > day),
  }));
  if (byDay.every((d) => d.entries.length === 0)) {
    return <p className="calendar-panel__empty">No events in these {SPAN} days.</p>;
  }
  return (
    <ol className="calendar-days">
      {byDay
        .filter((d) => d.entries.length > 0)
        .map(({ day, entries }) => (
          <li key={day} className="calendar-day">
            <h3 className="calendar-day__heading">
              {dayHeading.format(day)}
              {day === today && <span className="calendar-day__today">Today</span>}
            </h3>
            <ul className="calendar-day__events">
              {entries.map((entry) => (
                <Entry
                  key={entry.key}
                  entry={entry}
                  showProvider={showProvider}
                  onOpenApplication={onOpenApplication}
                />
              ))}
            </ul>
          </li>
        ))}
    </ol>
  );
}

function Entry({
  entry,
  showProvider,
  onOpenApplication,
}: {
  entry: CalendarEntry;
  showProvider: boolean;
  onOpenApplication: (id: number) => void;
}) {
  const interview = entry.interview;
  const role = interview ? [interview.company, interview.role].filter(Boolean).join(' — ') : null;
  // ReMa's own events are titled "Interview — Company — Role"; an event
  // the user renamed still names the application it belongs to.
  const titled = interview !== null && entry.title.includes(interview.company);
  return (
    <li className={`calendar-event${interview ? ' calendar-event--interview' : ''}`}>
      <span className="calendar-event__time">
        {entry.allDay ? 'All day' : formatTimeRange(entry.startAt, entry.endAt, null)}
      </span>
      <div className="calendar-event__main">
        <span className="calendar-event__title">{entry.title}</span>
        {interview && (
          <button
            type="button"
            className="link-button calendar-event__app"
            aria-label={titled ? `View application: ${role}` : undefined}
            onClick={() => onOpenApplication(interview.applicationId)}
          >
            {titled ? 'View application' : `Interview · ${role}`}
          </button>
        )}
        <span className="calendar-event__meta">
          {entry.provider && showProvider && <span>{PROVIDER_NAMES[entry.provider]}</span>}
          {interview && !interview.inCalendar && (
            <span className={interview.conflict ? 'calendar-event__warning' : undefined}>
              {interview.conflict ? 'Calendar conflict: not added' : 'Not in your calendar'}
            </span>
          )}
          {interview?.cancelled && <span>Cancelled</span>}
          {entry.location && !entry.location.startsWith('http') && <span>{entry.location}</span>}
        </span>
      </div>
      {entry.meetingUrl && (
        <button
          type="button"
          className="button button--secondary button--small calendar-event__join"
          onClick={() => void openExternalUrl(entry.meetingUrl ?? '').catch(() => {})}
        >
          <ExternalIcon className="button__icon" />
          Join
        </button>
      )}
    </li>
  );
}
