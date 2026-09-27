import { useState } from 'react';

import { useNavigation } from '../app/navigation';
import { ArrowLeftIcon, BriefcaseIcon, CalendarIcon, ExternalIcon, MailIcon } from '../components/icons';
import { PageContainer } from '../components/layout/PageContainer';
import { EmptyState, LoadingState } from '../components/ui/EmptyState';
import { useAction } from '../hooks/useAction';
import { useApplication, useApplications } from '../hooks/useApplications';
import { dataOr } from '../hooks/useAsyncData';
import { useConnectors } from '../hooks/useConnectors';
import { APPLICATION_STATUS_LABELS, formatDateTime, formatInTimezone, formatTimeRange } from '../lib/format';
import {
  addInterviewToCalendar,
  declineInterviewCalendar,
  setApplicationStatus,
  type ApplicationRow,
  type ApplicationStatus,
  type Correspondence,
  type EmailCategory,
  type InterviewView,
  type TimelineEntry,
  type UpdateSource,
} from '../services/applicationService';
import { openExternalUrl } from '../services/systemService';

const CATEGORY_LABELS: Record<EmailCategory, string> = {
  application_received: 'Application received',
  application_update: 'Application update',
  recruiter_message: 'Recruiter message',
  interview_request: 'Interview request',
  interview_confirmed: 'Interview confirmed',
  interview_rescheduled: 'Interview rescheduled',
  interview_cancelled: 'Interview cancelled',
  assessment_request: 'Assessment',
  action_required: 'Action required',
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

function StatusBadge({ status }: { status: ApplicationStatus }) {
  return <span className={`app-status app-status--${status}`}>{APPLICATION_STATUS_LABELS[status]}</span>;
}

function openMail(link: string | null) {
  if (link) void openExternalUrl(link).catch(() => {});
}

/** The job applications ReMa tracks from connected mail. */
export function ApplicationsPage({ applicationId }: { applicationId: number | null }) {
  const { navigate } = useNavigation();
  const overview = useApplications();
  const connectors = dataOr(useConnectors().state, null)?.connectors ?? [];
  const mailConnected = connectors.some((c) => c.kind === 'mail' && c.enabled);
  const open = (id: number | null) => navigate({ page: 'applications', applicationId: id });

  if (applicationId !== null) {
    return <ApplicationDetailView id={applicationId} onBack={() => open(null)} />;
  }

  const data = dataOr(overview.state, null);
  return (
    <PageContainer
      title="Applications"
      subtitle="Tracked from your connected mail. Every change shows where it came from."
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
          <div className="apps-summary" aria-label="Summary">
            <Stat value={data.summary.updatesToday} label="Updates today" />
            <Stat value={data.summary.interviewsScheduled} label="Interviews scheduled" />
            <Stat value={data.summary.actionRequired} label="Action required" attention />
            <Stat value={data.summary.offers} label="Offers" />
            <Stat value={data.summary.total} label="Applications" />
          </div>

          {data.needsReview.length > 0 && (
            <section className="section" aria-labelledby="review-heading">
              <h2 id="review-heading" className="section__title">
                Needs review
              </h2>
              <p className="section__description">
                ReMa was not sure what these emails mean, so it changed nothing. Check them yourself.
              </p>
              <ul className="apps-review">
                {data.needsReview.map((mail) => (
                  <MailRow key={`${mail.provider}:${mail.messageId}`} mail={mail} />
                ))}
              </ul>
            </section>
          )}

          {data.applications.length === 0 ? (
            <EmptyState
              framed
              icon={<BriefcaseIcon />}
              title={mailConnected ? 'No applications yet' : 'Connect your mailbox'}
              actions={
                !mailConnected && (
                  <button
                    type="button"
                    className="button button--primary"
                    onClick={() => navigate({ page: 'settings', focus: 'connectors' })}
                  >
                    Open Connectors
                  </button>
                )
              }
            >
              {mailConnected
                ? 'Applications appear here as ReMa finds job-related email: confirmations, interviews, rejections and offers.'
                : 'Connect Gmail or Outlook Mail and ReMa keeps track of your job applications from your email.'}
            </EmptyState>
          ) : (
            <div className="panel apps-table-wrap">
              <table className="job-table apps-table">
                <thead>
                  <tr>
                    <th scope="col">Company</th>
                    <th scope="col">Role</th>
                    <th scope="col">Status</th>
                    <th scope="col">Last update</th>
                    <th scope="col">Next</th>
                  </tr>
                </thead>
                <tbody>
                  {data.applications.map((row) => (
                    <tr key={row.id} className="apps-table__row" onClick={() => open(row.id)}>
                      <td className="job-table__company">
                        <button type="button" className="link-button apps-table__open" onClick={() => open(row.id)}>
                          {row.company}
                        </button>
                      </td>
                      <td>{row.role ?? '—'}</td>
                      <td>
                        <StatusBadge status={row.status} />
                      </td>
                      <td className="job-table__muted">{formatDateTime(row.lastUpdateAt)}</td>
                      <td>{nextStep(row)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </>
      )}
    </PageContainer>
  );
}

/** "Fri, 2 Oct, 14:00–15:00 (Europe/Vienna)" in the interview's own time zone. */
function interviewWhen(start: number, end: number, timezone: string | null): string {
  const options: Intl.DateTimeFormatOptions = { weekday: 'short', day: 'numeric', month: 'short' };
  let day: string;
  try {
    day = new Intl.DateTimeFormat(undefined, { ...options, timeZone: timezone ?? undefined }).format(start);
  } catch {
    day = new Intl.DateTimeFormat(undefined, options).format(start);
  }
  return `${day}, ${formatTimeRange(start, end, timezone)}${timezone ? ` (${timezone})` : ''}`;
}

function nextStep(row: ApplicationRow): string {
  if (row.interviewAt !== null) return `Interview ${formatInTimezone(row.interviewAt, row.interviewTimezone)}`;
  return row.nextAction ?? '—';
}

function Stat({ value, label, attention }: { value: number; label: string; attention?: boolean }) {
  return (
    <div className={`apps-summary__stat${attention && value > 0 ? ' apps-summary__stat--attention' : ''}`}>
      <span className="apps-summary__value">{value}</span>
      <span className="apps-summary__label">{label}</span>
    </div>
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
        <button type="button" className="button button--ghost button--small" onClick={() => openMail(mail.webLink)}>
          <ExternalIcon className="button__icon" />
          Open in {mail.provider === 'microsoft' ? 'Outlook' : 'Gmail'}
        </button>
      )}
    </li>
  );
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
            <StatusBadge status={app.status} />
            {app.nextAction && <span className="apps-detail__next">Next: {app.nextAction}</span>}
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

  const when = i.startAt !== null && i.endAt !== null ? interviewWhen(i.startAt, i.endAt, i.timezone) : null;

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
            <button type="button" className="link-button" onClick={() => openMail(i.meetingUrl)}>
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
              Conflicts with “{c.title}” ({formatTimeRange(c.startAt, c.endAt, i.timezone)})
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
                {interviewWhen(slot.startAt, slot.endAt, null)} —{' '}
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
