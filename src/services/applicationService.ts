import { commands, type ApplicationDetail, type ApplicationStatus, type ApplicationsOverview } from '../generated/bindings';
import { callBackend } from './ipc';

export type {
  ApplicationDetail,
  ApplicationRow,
  ApplicationStatus,
  ApplicationsOverview,
  ApplicationsSummary,
  CalendarState,
  Correspondence,
  EmailCategory,
  InterviewView,
  NotificationItem,
  ProposedSlot,
  TimelineEntry,
  UpdateSource,
} from '../generated/bindings';

export function getApplications(): Promise<ApplicationsOverview> {
  return callBackend(() => commands.getApplications());
}

export function getApplication(id: number): Promise<ApplicationDetail> {
  return callBackend(() => commands.getApplication(id));
}

export function setApplicationStatus(id: number, status: ApplicationStatus, note: string | null): Promise<ApplicationDetail> {
  return callBackend(() => commands.setApplicationStatus(id, status, note));
}

/** Adds a confirmed interview to the calendar; a conflict needs `allowConflict`. */
export function addInterviewToCalendar(interviewId: number, allowConflict: boolean): Promise<ApplicationDetail> {
  return callBackend(() => commands.addInterviewToCalendar(interviewId, allowConflict));
}

export function declineInterviewCalendar(interviewId: number): Promise<ApplicationDetail> {
  return callBackend(() => commands.declineInterviewCalendar(interviewId));
}

export function listNotifications() {
  return callBackend(() => commands.listNotifications());
}

/** Marks the given notifications (all without `ids`) as read. */
export function markNotificationsRead(ids: number[] | null): Promise<null> {
  return callBackend(() => commands.markNotificationsRead(ids));
}

export function clearNotifications(): Promise<null> {
  return callBackend(() => commands.clearNotifications());
}
