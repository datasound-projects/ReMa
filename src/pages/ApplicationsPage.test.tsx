// @vitest-environment jsdom
import '../test/dom';

import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { NavigationContext, type View } from '../app/navigation';
import type {
  ApplicationDetail,
  ApplicationRow,
  ApplicationsOverview,
  InterviewView,
} from '../services/applicationService';
import { ApplicationsPage } from './ApplicationsPage';

const mocks = vi.hoisted(() => ({
  getApplications: vi.fn(),
  getApplication: vi.fn(),
  setApplicationStatus: vi.fn(),
  addInterviewToCalendar: vi.fn(),
  declineInterviewCalendar: vi.fn(),
  listNotifications: vi.fn(),
  markNotificationsRead: vi.fn(),
  clearNotifications: vi.fn(),
}));

vi.mock('../services/applicationService', () => mocks);
vi.mock('../services/connectorService', () => ({ getConnectors: vi.fn(() => new Promise(() => {})) }));
vi.mock('../services/systemService', () => ({ openExternalUrl: vi.fn(() => Promise.resolve(null)) }));

const DAY = 86_400_000;
const start = Date.now() + 3 * DAY;

function row(patch: Partial<ApplicationRow> & Pick<ApplicationRow, 'id' | 'company'>): ApplicationRow {
  return {
    role: 'Engineer',
    status: 'in_process',
    section: 'in_progress',
    statusLabel: 'In review',
    lastUpdateAt: Date.now() - DAY,
    latestUpdate: 'Recruiting team is reviewing your application.',
    nextAction: null,
    rejectionReason: null,
    interviewAt: null,
    interviewTimezone: null,
    meetingUrl: null,
    calendarConflict: false,
    mailProvider: 'google',
    ...patch,
  };
}

const globex = row({
  id: 7,
  company: 'Globex',
  role: 'Data Engineer',
  status: 'upcoming_interview',
  section: 'interviews_confirmed',
  statusLabel: 'Interview confirmed',
  lastUpdateAt: Date.now() - 3_600_000,
  latestUpdate: 'Interview confirmed for Sep 29 at 10:00 CEST.',
  interviewAt: start,
  interviewTimezone: 'CEST',
  meetingUrl: 'https://meet.google.com/abc-defg-hij',
});

const overview: ApplicationsOverview = {
  applications: [
    globex,
    row({
      id: 8,
      company: 'Umbrella',
      status: 'needs_action',
      section: 'needs_action',
      statusLabel: 'Interview requested',
      lastUpdateAt: Date.now() - 7_200_000,
      latestUpdate: 'Choose an interview slot from the proposed times.',
    }),
    row({ id: 9, company: 'Stark', lastUpdateAt: Date.now() - 2 * DAY }),
    row({
      id: 10,
      company: 'Initech',
      status: 'rejected',
      section: 'rejected',
      statusLabel: 'Rejected',
      latestUpdate: 'No reason provided.',
    }),
  ],
  tracking: { taskId: 1, enabled: true, mailConnected: true, lastRunAt: Date.now() - 600_000, nextRunAt: null },
  needsReview: [],
};

function interview(patch: Partial<InterviewView>): InterviewView {
  return {
    id: 3,
    state: 'confirmed',
    interviewType: 'Technical interview',
    startAt: start,
    endAt: start + 3_600_000,
    timezone: 'Europe/Vienna',
    timeText: 'Sep 29 at 10:00 CEST',
    location: null,
    meetingUrl: 'https://meet.example.com/globex-1',
    participants: [],
    reviewReason: null,
    calendarState: 'proposed',
    calendarProvider: 'google',
    conflicts: [],
    proposedSlots: [],
    ...patch,
  };
}

function detail(i: InterviewView): ApplicationDetail {
  return {
    application: globex,
    reference: null,
    interviews: [i],
    timeline: [
      {
        id: 2,
        source: 'gmail',
        change: 'Application changed to Interview confirmed',
        status: 'upcoming_interview',
        previousStatus: 'confirmed',
        category: 'interview_confirmed',
        confidence: 0.98,
        summary: 'Technical interview confirmed.',
        occurredAt: Date.now() - 3_600_000,
        createdAt: Date.now(),
      },
    ],
    correspondence: [],
  };
}

function show(view: View & { page: 'applications' }, navigate = vi.fn()) {
  render(
    <NavigationContext value={{ view, navigate }}>
      <ApplicationsPage applicationId={view.applicationId ?? null} />
    </NavigationContext>,
  );
  return navigate;
}

beforeEach(() => {
  vi.clearAllMocks();
  mocks.getApplications.mockResolvedValue(overview);
});

describe('Applications', () => {
  it('shows exactly five sections, each application in one, with the required columns', async () => {
    const navigate = show({ page: 'applications' });
    expect(await screen.findByText('Globex')).toBeTruthy();
    const sections = screen.getAllByRole('region');
    expect(sections.map((s) => s.getAttribute('aria-label'))).toEqual([
      'Interviews Confirmed',
      'Applications Confirmed',
      'Needs Your Action',
      'Applications In Progress',
      'Rejected',
    ]);
    const [interviews, confirmed, action, progress, rejected] = sections as [
      HTMLElement,
      HTMLElement,
      HTMLElement,
      HTMLElement,
      HTMLElement,
    ];
    expect(within(interviews).getByText('Globex')).toBeTruthy();
    expect(within(interviews).getByText('Interview confirmed for Sep 29 at 10:00 CEST.')).toBeTruthy();
    expect(within(confirmed).getByText('No confirmed applications waiting for news.')).toBeTruthy();
    expect(within(action).getByText('Choose an interview slot from the proposed times.')).toBeTruthy();
    expect(within(progress).getByText('Stark')).toBeTruthy();
    expect(within(rejected).getByText('No reason provided.')).toBeTruthy();
    expect(screen.getAllByText('Globex')).toHaveLength(1);
    const headers = within(interviews)
      .getAllByRole('columnheader')
      .map((h) => h.textContent);
    expect(headers).toEqual(['Date (latest msg)', 'Company', 'Role', 'Status', 'Interview']);
    expect(within(rejected).getByRole('columnheader', { name: 'Rejection reason' })).toBeTruthy();
    fireEvent.click(within(interviews).getByRole('button', { name: 'Globex' }));
    expect(navigate).toHaveBeenCalledWith({ page: 'applications', applicationId: 7 });
  });

  it('collapses and expands each section on its own', async () => {
    show({ page: 'applications' });
    const toggle = await screen.findByRole('button', { name: /Rejected/ });
    expect(toggle.getAttribute('aria-expanded')).toBe('true');
    fireEvent.click(toggle);
    expect(toggle.getAttribute('aria-expanded')).toBe('false');
    expect(screen.queryByText('No reason provided.')).toBeNull();
    expect(screen.getByText('Globex')).toBeTruthy();
    fireEvent.click(toggle);
    expect(screen.getByText('No reason provided.')).toBeTruthy();
  });

  it('opens meeting links directly', async () => {
    const { openExternalUrl } = await import('../services/systemService');
    show({ page: 'applications' });
    fireEvent.click(await screen.findByRole('button', { name: 'Meeting link' }));
    expect(openExternalUrl).toHaveBeenCalledWith('https://meet.google.com/abc-defg-hij');
  });

  it('says when mail is not tracked, without scanning anything', async () => {
    mocks.getApplications.mockResolvedValue({
      ...overview,
      applications: [],
      tracking: { taskId: null, enabled: false, mailConnected: true, lastRunAt: null, nextRunAt: null },
    });
    const navigate = show({ page: 'applications' });
    expect(await screen.findByText(/ReMa is not reading your mail/)).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Scheduled Tasks' }));
    expect(navigate).toHaveBeenCalledWith({ page: 'tasks' });
    expect(screen.getAllByRole('region')).toHaveLength(5);
  });

  it('shows where each change came from', async () => {
    mocks.getApplication.mockResolvedValue(detail(interview({})));
    show({ page: 'applications', applicationId: 7 });
    expect(await screen.findByText('Application changed to Interview confirmed')).toBeTruthy();
    expect(screen.getByText(/Source: Gmail message · Confidence: 98%/)).toBeTruthy();
  });

  it('can still add a confirmed interview by hand, or decline it', async () => {
    mocks.getApplication.mockResolvedValue(detail(interview({})));
    mocks.addInterviewToCalendar.mockResolvedValue(detail(interview({ calendarState: 'created' })));
    mocks.declineInterviewCalendar.mockResolvedValue(detail(interview({ calendarState: 'declined' })));
    show({ page: 'applications', applicationId: 7 });
    fireEvent.click(await screen.findByRole('button', { name: 'Add to calendar' }));
    await waitFor(() => expect(mocks.addInterviewToCalendar).toHaveBeenCalledWith(3, false));
    fireEvent.click(screen.getByRole('button', { name: 'Don’t add' }));
    await waitFor(() => expect(mocks.declineInterviewCalendar).toHaveBeenCalledWith(3));
  });

  it('shows conflicts and adds only with "Add anyway"', async () => {
    mocks.getApplication.mockResolvedValue(
      detail(
        interview({
          calendarState: 'conflict',
          conflicts: [{ title: 'Dentist', startAt: start + 1_800_000, endAt: start + 5_400_000 }],
        }),
      ),
    );
    mocks.addInterviewToCalendar.mockResolvedValue(detail(interview({ calendarState: 'created' })));
    show({ page: 'applications', applicationId: 7 });
    expect(await screen.findByText(/Conflicts with “Dentist”/)).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Add to calendar' })).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Add anyway' }));
    await waitFor(() => expect(mocks.addInterviewToCalendar).toHaveBeenCalledWith(3, true));
  });
});
