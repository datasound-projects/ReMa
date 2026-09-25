import type {
  ApplicationRow,
  CalendarItem,
  CalendarOutcome,
  CalendarReport,
  JobRunReport,
} from '../../generated/bindings';
import {
  APPLICATION_STATUS_LABELS,
  formatDateTime,
  formatInTimezone,
  formatTimeRange,
} from '../../lib/format';

const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

/** The result of a job-application run: counts, overview table, Calendar. */
export function JobReport({ report }: { report: JobRunReport }) {
  const counts = [
    plural(report.applicationsUpdated, 'application updated', 'applications updated'),
    plural(report.relevantEmails, 'new relevant email', 'new relevant emails'),
    plural(report.upcomingInterviews, 'upcoming interview', 'upcoming interviews'),
    plural(report.needsAction, 'needs action', 'need action'),
    plural(report.newRejections, 'new rejection', 'new rejections'),
  ];
  if (report.calendar) {
    counts.push(plural(report.calendar.conflicts, 'calendar conflict', 'calendar conflicts'));
  }

  return (
    <div className="job-report">
      <h3 className="job-report__title">Job Application Update</h3>
      <p className="job-report__counts">{counts.join(' · ')}</p>
      <p className="job-report__window">
        Emails since {formatDateTime(report.windowStart)} · {report.emailsChecked} checked,{' '}
        {report.newEmails} new
        {report.deferredEmails > 0 && ` · ${report.deferredEmails} left for the next run`}
      </p>

      {report.applications.length === 0 ? (
        <p className="empty-note">No job applications found in this period.</p>
      ) : (
        <ApplicationTable rows={report.applications} />
      )}

      {report.calendar && <CalendarBlock calendar={report.calendar} />}

      {report.issues.length > 0 && (
        <div className="job-report__issues">
          <h4 className="job-report__subtitle">Notes</h4>
          <ul>
            {report.issues.map((issue, i) => (
              <li key={i}>{issue}</li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}

function nextStep(row: ApplicationRow): string {
  if (row.status === 'upcoming_interview' && row.interviewAt !== null) {
    return `Interview ${formatInTimezone(row.interviewAt, row.interviewTimezone)}`;
  }
  return row.nextAction ?? '—';
}

function ApplicationTable({ rows }: { rows: ApplicationRow[] }) {
  return (
    <div className="job-table-wrap">
      <table className="job-table">
        <thead>
          <tr>
            <th scope="col">Company</th>
            <th scope="col">Position</th>
            <th scope="col">Status</th>
            <th scope="col">Last Update</th>
            <th scope="col">Next Action</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => (
            <tr key={row.id}>
              <td className="job-table__company">{row.company}</td>
              <td>{row.role ?? '—'}</td>
              <td>
                <span className={`app-status app-status--${row.status}`}>
                  {APPLICATION_STATUS_LABELS[row.status]}
                </span>
              </td>
              <td className="job-table__muted">{formatDateTime(row.lastUpdateAt)}</td>
              <td>{nextStep(row)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

const OUTCOME_LABELS: Record<CalendarOutcome, string> = {
  created: 'Created',
  updated: 'Updated',
  unchanged: 'In Calendar',
  cancelled: 'Marked cancelled',
  removed: 'Deleted in Calendar',
  needs_review: 'Needs review',
};

function CalendarBlock({ calendar }: { calendar: CalendarReport }) {
  const counts = [
    `Created ${calendar.created}`,
    `Updated ${calendar.updated}`,
    `Unchanged ${calendar.unchanged}`,
    calendar.cancelled > 0 && `Cancelled ${calendar.cancelled}`,
    `Conflicts ${calendar.conflicts}`,
    `Needs Review ${calendar.needsReview}`,
  ].filter(Boolean);
  return (
    <div className="job-report__calendar">
      <h4 className="job-report__subtitle">Calendar</h4>
      <p className="job-report__counts">{counts.join(' · ')}</p>
      {calendar.items.length > 0 && (
        <ul className="calendar-items">
          {calendar.items.map((item, i) => (
            <CalendarRow key={i} item={item} />
          ))}
        </ul>
      )}
    </div>
  );
}

function CalendarRow({ item }: { item: CalendarItem }) {
  const title = item.role ? `${item.role} — ${item.company}` : item.company;
  return (
    <li className="calendar-items__item">
      <div className="calendar-items__row">
        <span className="calendar-items__title">{title}</span>
        {item.startAt !== null && (
          <span className="job-table__muted">{formatInTimezone(item.startAt, item.timezone)}</span>
        )}
        <span className={`calendar-outcome calendar-outcome--${item.outcome}`}>
          {OUTCOME_LABELS[item.outcome]}
        </span>
      </div>
      {item.note && <p className="calendar-items__note">{item.note}</p>}
      {item.conflicts.map((conflict, i) => (
        // Same timezone as the interview above.
        <p key={i} className="calendar-items__conflict">
          Overlaps “{conflict.title}” ({formatTimeRange(conflict.startAt, conflict.endAt, item.timezone)})
        </p>
      ))}
    </li>
  );
}
