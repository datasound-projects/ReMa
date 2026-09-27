import {
  commands,
  type ApplicationDetail,
  type ApplicationStatus,
  type ApplicationsOverview,
  type CalendarView,
} from '../generated/bindings';
import { callBackend } from './ipc';

export type {
  ApplicationDetail,
  ApplicationRow,
  ApplicationSection,
  ApplicationStatus,
  ApplicationsOverview,
  CalendarEntry,
  CalendarSource,
  CalendarState,
  CalendarView,
  Correspondence,
  EmailCategory,
  InterviewView,
  NotificationItem,
  ProposedSlot,
  InterviewLink,
  TimelineEntry,
  TrackingStatus,
  UpdateSource,
} from '../generated/bindings';

export function getApplications(): Promise<ApplicationsOverview> {
  return callBackend(() => commands.getApplications());
}

/** The in-app calendar: connected calendars' events (read live) with ReMa's interviews. */
export function getCalendar(start: number, end: number): Promise<CalendarView> {
  return callBackend(() => commands.getCalendar(start, end));
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
