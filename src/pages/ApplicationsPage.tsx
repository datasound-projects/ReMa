import { useState } from 'react';

import { useNavigation } from '../app/navigation';
import {
  ArrowLeftIcon,
  CalendarIcon,
  ChevronDownIcon,
  ChevronRightIcon,
  ExternalIcon,
  MailCalendarIcon,
  MailIcon,
} from '../components/icons';
import { PageContainer } from '../components/layout/PageContainer';
import { LoadingState } from '../components/ui/EmptyState';
import { useAction } from '../hooks/useAction';
import { useApplication, useApplications } from '../hooks/useApplications';
import { dataOr } from '../hooks/useAsyncData';
import { APPLICATION_STATUS_LABELS, formatDate, formatDateTime, formatRelative, formatTimeRange } from '../lib/format';
import {
  addInterviewToCalendar,
  declineInterviewCalendar,
  setApplicationStatus,
  type ApplicationRow,
  type ApplicationSection,
  type ApplicationStatus,
  type Correspondence,
  type EmailCategory,
  type InterviewView,
  type TimelineEntry,
  type TrackingStatus,
  type UpdateSource,
} from '../services/applicationService';
import { openExternalUrl } from '../services/systemService';

const CATEGORY_LABELS: Record<EmailCategory, string> = {
  application_confirmed: 'Application confirmed',
  application_update: 'Application update',
  needs_action: 'Action requested',
  recruiter_message: 'Recruiter message',
  interview_request: 'Interview requested',
  interview_confirmed: 'Interview confirmed',
  interview_rescheduled: 'Interview rescheduled',
  interview_cancelled: 'Interview cancelled',
  assessment_request: 'Assessment requested',
  rejection: 'Rejection',
  offer: 'Offer',
  other_job_related: 'Other job-related',
  not_job_related: 'Not job-related',
};

const SOURCE_LABELS: Record<UpdateSource, string> = {
  gmail: 'Gmail message',
  outlook: 'Outlook message',
  google_calendar: 'Google Calendar',
  outlook_calendar: 'Outlook Calendar',
  user: 'You',
  assistant: 'ReMa assistant (approved by you)',
};

const CALENDAR_NAMES: Record<string, string> = { google: 'Google Calendar', microsoft: 'Outlook Calendar' };

const STATUSES = Object.keys(APPLICATION_STATUS_LABELS) as ApplicationStatus[];

/** The five sections, in page order. Each application is in exactly one. */
const SECTIONS: { id: ApplicationSection; title: string; lastColumn: string; empty: string }[] = [
  {
    id: 'interviews_confirmed',
    title: 'Interviews Confirmed',
    lastColumn: 'Interview',
    empty: 'No confirmed interviews.',
  },
  {
    id: 'applications_confirmed',
    title: 'Applications Confirmed',
    lastColumn: 'Latest update',
    empty: 'No confirmed applications waiting for news.',
  },
  { id: 'needs_action', title: 'Needs Your Action', lastColumn: 'Requested action', empty: 'Nothing needs your action.' },
  {
    id: 'in_progress',
    title: 'Applications In Progress',
    lastColumn: 'Latest update',
    empty: 'No applications in progress.',
  },
  { id: 'rejected', title: 'Rejected', lastColumn: 'Rejection reason', empty: 'No rejections.' },
];

const COLLAPSED_KEY = 'rema.applications.collapsed';

/** Which sections the user collapsed (remembered on this computer only). */
function useCollapsed() {
  const [collapsed, setCollapsed] = useState<ApplicationSection[]>(() => {
    try {
      const saved = JSON.parse(localStorage.getItem(COLLAPSED_KEY) ?? '[]') as unknown;
      return Array.isArray(saved) ? (saved as ApplicationSection[]) : [];
    } catch {
      return [];
    }
  });
  const toggle = (id: ApplicationSection) => {
    const next = collapsed.includes(id) ? collapsed.filter((c) => c !== id) : [...collapsed, id];
    setCollapsed(next);
    try {
      localStorage.setItem(COLLAPSED_KEY, JSON.stringify(next));
    } catch {
      // Not remembered; the page still works.
    }
  };
  return { collapsed, toggle };
}

function openLink(link: string | null) {
  if (link) void openExternalUrl(link).catch(() => {});
}

/**
 * The job applications ReMa tracks: five sections for the current state,
 * each application in exactly one, latest email first.
 */
export function ApplicationsPage({ applicationId }: { applicationId: number | null }) {
  const { navigate } = useNavigation();
  const overview = useApplications();
  const { collapsed, toggle } = useCollapsed();
  const open = (id: number | null) => navigate({ page: 'applications', applicationId: id });

  if (applicationId !== null) {
    return <ApplicationDetailView id={applicationId} onBack={() => open(null)} />;
  }

  const data = dataOr(overview.state, null);
  return (
    <PageContainer
      title="Applications"
      subtitle="The current state of each job application, from your job email."
      width="wide"
    >
      {overview.state.status === 'loading' && <LoadingState />}
      {overview.state.status === 'error' && (
        <p className="form-error" role="alert">
          {overview.state.error.message}
        </p>
      )}
      {data && (
        <>
          <TrackingNotice tracking={data.tracking} />
          <div className="apps-sections">
            {SECTIONS.map((section) => (
              <SectionCard
                key={section.id}
                section={section}
                rows={data.applications.filter((row) => row.section === section.id)}
                expanded={!collapsed.includes(section.id)}
                onToggle={() => toggle(section.id)}
                onOpen={open}
              />
            ))}
          </div>
        </>
      )}
    </PageContainer>
  );
}

/** Where the rows come from: only "Job Mail & Interview Sync" reads mail. */
function TrackingNotice({ tracking }: { tracking: TrackingStatus }) {
  const { navigate } = useNavigation();
  if (tracking.enabled) {
    return (
      <p className="apps-tracking apps-tracking--on">
        <MailCalendarIcon className="apps-tracking__icon" aria-hidden="true" />
        <span>
          Updated by Job Mail &amp; Interview Sync
          {tracking.lastRunAt !== null && ` · last run ${formatRelative(tracking.lastRunAt)}`}
          {tracking.nextRunAt !== null && ` · next run ${formatDateTime(tracking.nextRunAt)}`}
        </span>
      </p>
    );
  }
  return (
    <div className="apps-tracking apps-tracking--off" role="note">
      <MailCalendarIcon className="apps-tracking__icon" aria-hidden="true" />
      <p className="apps-tracking__text">
        {tracking.mailConnected
          ? 'ReMa is not reading your mail. Turn on Job Mail & Interview Sync in Scheduled Tasks to keep these sections up to date.'
          : 'Connect Gmail or Outlook Mail in Settings → Connectors, then turn on Job Mail & Interview Sync in Scheduled Tasks.'}
      </p>
      <button
        type="button"
        className="button button--secondary button--small"
        onClick={() =>
          tracking.mailConnected ? navigate({ page: 'tasks' }) : navigate({ page: 'settings', focus: 'connectors' })
        }
      >
        {tracking.mailConnected ? 'Scheduled Tasks' : 'Open Connectors'}
      </button>
    </div>
  );
}

function SectionCard({
  section,
  rows,
  expanded,
  onToggle,
  onOpen,
}: {
  section: (typeof SECTIONS)[number];
  rows: ApplicationRow[];
  expanded: boolean;
  onToggle: () => void;
  onOpen: (id: number) => void;
}) {
  const bodyId = `apps-section-${section.id}`;
  return (
    <section className={`apps-section apps-section--${section.id}`} aria-label={section.title}>
      <h2 className="apps-section__heading">
        <button
          type="button"
          className="apps-section__toggle"
          aria-expanded={expanded}
          aria-controls={bodyId}
          onClick={onToggle}
        >
          {expanded ? (
            <ChevronDownIcon className="apps-section__chevron" aria-hidden="true" />
          ) : (
            <ChevronRightIcon className="apps-section__chevron" aria-hidden="true" />
          )}
          <span className="apps-section__title">{section.title}</span>
          <span
            className={`apps-section__count${section.id === 'needs_action' && rows.length > 0 ? ' apps-section__count--attention' : ''}`}
            aria-label={`${rows.length} applications`}
          >
            {rows.length}
          </span>
        </button>
      </h2>
      {expanded && (
        <div className="apps-section__body" id={bodyId}>
          {rows.length === 0 ? (
            <p className="apps-section__empty">{section.empty}</p>
          ) : (
            <div className="apps-section__scroll">
              <table className="job-table apps-table">
                <thead>
                  <tr>
                    <th scope="col" className="apps-table__date">
                      Date (latest msg)
                    </th>
                    <th scope="col">Company</th>
                    <th scope="col">Role</th>
                    <th scope="col">Status</th>
                    <th scope="col" className="apps-table__update">
                      {section.lastColumn}
                    </th>
                  </tr>
                </thead>
                <tbody>
                  {rows.map((row) => (
                    <Row key={row.id} row={row} onOpen={() => onOpen(row.id)} />
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </div>
      )}
    </section>
  );
}

function Row({ row, onOpen }: { row: ApplicationRow; onOpen: () => void }) {
  return (
    <tr className="apps-table__row" onClick={onOpen}>
      <td className="job-table__muted apps-table__date">{formatDate(row.lastUpdateAt)}</td>
      <td className="job-table__company">
        <button
          type="button"
          className="link-button apps-table__open"
          onClick={(e) => {
            e.stopPropagation();
            onOpen();
          }}
        >
          {row.company}
        </button>
      </td>
      <td>{row.role ?? '—'}</td>
      <td>
        <span className={`app-status app-status--${row.status}`}>{row.statusLabel}</span>
      </td>
      <td className="apps-table__update">
        <span className="apps-table__update-text">{row.latestUpdate}</span>
        {row.calendarConflict && <span className="calendar-outcome calendar-outcome--conflict">Calendar conflict</span>}
        {row.meetingUrl && (
          <button
            type="button"
            className="button button--ghost button--small apps-table__join"
            onClick={(e) => {
              e.stopPropagation();
              openLink(row.meetingUrl);
            }}
          >
            <ExternalIcon className="button__icon" />
            Meeting link
          </button>
        )}
      </td>
    </tr>
  );
}

function MailRow({ mail }: { mail: Correspondence }) {
  return (
    <li className="apps-mail">
      <MailIcon className="apps-mail__icon" aria-hidden="true" />
      <div className="apps-mail__text">
        <span className="apps-mail__subject">{mail.subject ?? '(no subject)'}</span>
        <span className="apps-mail__meta">
          {[mail.sender, formatDateTime(mail.receivedAt), mail.category && CATEGORY_LABELS[mail.category]]
            .filter(Boolean)
            .join(' · ')}
          {mail.confidence !== null && ` · Confidence ${Math.round(mail.confidence * 100)}%`}
        </span>
      </div>
      {mail.webLink && (
        <button type="button" className="button button--ghost button--small" onClick={() => openLink(mail.webLink)}>
          <ExternalIcon className="button__icon" />
          Open in {mail.provider === 'microsoft' ? 'Outlook' : 'Gmail'}
        </button>
      )}
    </li>
  );
}

/** "Fri, 2 Oct, 14:00–15:00" in local time (proposed slots). */
function slotWhen(start: number, end: number): string {
  const day = new Intl.DateTimeFormat(undefined, { weekday: 'short', day: 'numeric', month: 'short' }).format(start);
  return `${day}, ${formatTimeRange(start, end, null)}`;
}

function ApplicationDetailView({ id, onBack }: { id: number; onBack: () => void }) {
  const detail = useApplication(id);
  const action = useAction();
  const data = dataOr(detail.state, null);
  const app = data?.application;

  return (
    <PageContainer
      title={
        <span className="apps-detail__title">
          <button type="button" className="icon-button icon-button--small" aria-label="Back to applications" onClick={onBack}>
            <ArrowLeftIcon />
          </button>
          {app ? app.company : 'Application'}
        </span>
      }
      subtitle={app ? [app.role, data?.reference && `Reference ${data.reference}`].filter(Boolean).join(' · ') : undefined}
      width="wide"
      actions={
        app && (
          <label className="apps-detail__status">
            <span className="sr-only">Status</span>
            <select
              className="input input--auto input--small"
              value={app.status}
              disabled={action.busy}
              onChange={(e) =>
                void action.run(() => setApplicationStatus(app.id, e.target.value as ApplicationStatus, null))
              }
            >
              {STATUSES.map((s) => (
                <option key={s} value={s}>
                  {APPLICATION_STATUS_LABELS[s]}
                </option>
              ))}
            </select>
          </label>
        )
      }
    >
      {detail.state.status === 'loading' && <LoadingState />}
      {detail.state.status === 'error' && (
        <p className="form-error" role="alert">
          {detail.state.error.message}
        </p>
      )}
      {action.error && <p className="form-error">{action.error}</p>}
      {data && app && (
        <div className="apps-detail">
          <div className="apps-detail__summary">
            <span className={`app-status app-status--${app.status}`}>{app.statusLabel}</span>
            <span className="apps-detail__next">{app.latestUpdate}</span>
          </div>

          {data.interviews.length > 0 && (
            <section className="section" aria-labelledby="interviews-heading">
              <h2 id="interviews-heading" className="section__title">
                Interviews
              </h2>
              <div className="apps-interviews">
                {data.interviews.map((interview) => (
                  <InterviewCard key={interview.id} interview={interview} />
                ))}
              </div>
            </section>
          )}

          <section className="section" aria-labelledby="timeline-heading">
            <h2 id="timeline-heading" className="section__title">
              Timeline
            </h2>
            <ol className="apps-timeline">
              {data.timeline.map((entry) => (
                <TimelineItem key={entry.id} entry={entry} />
              ))}
            </ol>
          </section>

          {data.correspondence.length > 0 && (
            <section className="section" aria-labelledby="mail-heading">
              <h2 id="mail-heading" className="section__title">
                Correspondence
              </h2>
              <ul className="apps-review">
                {data.correspondence.map((mail) => (
                  <MailRow key={`${mail.provider}:${mail.messageId}`} mail={mail} />
                ))}
              </ul>
            </section>
          )}
        </div>
      )}
    </PageContainer>
  );
}

function TimelineItem({ entry }: { entry: TimelineEntry }) {
  return (
    <li className="apps-timeline__item">
      <span className="apps-timeline__dot" aria-hidden="true" />
      <div className="apps-timeline__body">
        <span className="apps-timeline__change">{entry.change}</span>
        <span className="apps-timeline__meta">
          Source: {SOURCE_LABELS[entry.source]}
          {entry.confidence !== null && ` · Confidence: ${Math.round(entry.confidence * 100)}%`}
          {' · '}
          {formatDateTime(entry.occurredAt)}
        </span>
        {entry.summary && <span className="apps-timeline__summary">{entry.summary}</span>}
      </div>
    </li>
  );
}

function InterviewCard({ interview: i }: { interview: InterviewView }) {
  const action = useAction();
  const calendar = (i.calendarProvider && CALENDAR_NAMES[i.calendarProvider]) || 'your calendar';
  // When the card was shown (render stays pure).
  const [shownAt] = useState(() => Date.now());
  const upcoming = i.endAt !== null && i.endAt > shownAt;
  const confirmed = i.state === 'confirmed';

  // A conflict found while adding marks the interview (calendarState
  // "conflict"), which offers "Add anyway".
  const add = (allowConflict: boolean) => void action.run(() => addInterviewToCalendar(i.id, allowConflict));

  const when = i.timeText;

  return (
    <div className="apps-interview">
      <div className="apps-interview__head">
        <CalendarIcon className="apps-interview__icon" aria-hidden="true" />
        <div className="apps-interview__text">
          <span className="apps-interview__title">
            {i.interviewType ?? 'Interview'}
            {i.state === 'cancelled' && ' — cancelled'}
          </span>
          {when && <span className="apps-interview__when">{when}</span>}
        </div>
        <CalendarBadge interview={i} calendar={calendar} />
      </div>
      {(i.meetingUrl || i.location) && (
        <p className="apps-interview__line">
          {i.location && <span>{i.location}</span>}
          {i.meetingUrl && (
            <button type="button" className="link-button" onClick={() => openLink(i.meetingUrl)}>
              Meeting link
            </button>
          )}
        </p>
      )}
      {i.participants.length > 0 && <p className="apps-interview__line">With {i.participants.join(', ')}</p>}
      {i.reviewReason && <p className="apps-interview__review">Needs review: {i.reviewReason}</p>}
      {i.conflicts.length > 0 && (
        <ul className="apps-interview__conflicts">
          {i.conflicts.map((c, n) => (
            <li key={n}>
              Conflicts with “{c.title}” ({formatTimeRange(c.startAt, c.endAt, null)})
            </li>
          ))}
        </ul>
      )}
      {i.proposedSlots.length > 0 && (
        <div className="apps-interview__slots">
          <span className="apps-interview__label">Proposed times</span>
          <ul>
            {i.proposedSlots.map((slot, n) => (
              <li key={n}>
                {slotWhen(slot.startAt, slot.endAt)} —{' '}
                {slot.available === true
                  ? 'you are free'
                  : slot.available === false
                    ? `conflicts with ${slot.conflicts.map((c) => `“${c.title}”`).join(', ')}`
                    : 'calendar not checked'}
              </li>
            ))}
          </ul>
        </div>
      )}
      {confirmed && upcoming && i.calendarState !== 'created' && (
        <div className="apps-interview__actions">
          {i.calendarState === 'conflict' ? (
            <button
              type="button"
              className="button button--secondary button--small"
              disabled={action.busy}
              onClick={() => add(true)}
            >
              Add anyway
            </button>
          ) : (
            <button
              type="button"
              className="button button--primary button--small"
              disabled={action.busy}
              onClick={() => add(false)}
            >
              Add to calendar
            </button>
          )}
          {i.calendarState !== 'declined' && (
            <button
              type="button"
              className="button button--ghost button--small"
              disabled={action.busy}
              onClick={() => void action.run(() => declineInterviewCalendar(i.id))}
            >
              Don’t add
            </button>
          )}
        </div>
      )}
      {action.error && <p className="form-error">{action.error}</p>}
    </div>
  );
}

function CalendarBadge({ interview: i, calendar }: { interview: InterviewView; calendar: string }) {
  if (i.state !== 'confirmed') return null;
  const labels: Record<InterviewView['calendarState'], string | null> = {
    none: null,
    proposed: 'Not in calendar yet',
    conflict: 'Conflict',
    created: `In ${calendar}`,
    declined: 'Not added',
    removed: `Deleted in ${calendar}`,
  };
  const label = labels[i.calendarState];
  return label ? <span className={`calendar-outcome calendar-outcome--${badgeTone(i.calendarState)}`}>{label}</span> : null;
}

function badgeTone(state: InterviewView['calendarState']): string {
  switch (state) {
    case 'created':
      return 'created';
    case 'conflict':
      return 'conflict';
    case 'proposed':
      return 'proposed';
    default:
      return 'unchanged';
  }
}
