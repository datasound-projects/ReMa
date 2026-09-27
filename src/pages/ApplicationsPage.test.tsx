// @vitest-environment jsdom
import '../test/dom';

import { fireEvent, render, screen, waitFor } from '@testing-library/react';
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

const globex: ApplicationRow = {
  id: 7,
  company: 'Globex',
  role: 'Data Engineer',
  status: 'upcoming_interview',
  lastUpdateAt: Date.now() - 3_600_000,
  nextAction: null,
  interviewAt: start,
  interviewTimezone: 'Europe/Vienna',
};

const overview: ApplicationsOverview = {
  summary: { updatesToday: 2, interviewsScheduled: 1, actionRequired: 1, offers: 0, total: 2 },
  applications: [
    globex,
    {
      id: 8,
      company: 'Umbrella',
      role: null,
      status: 'needs_action',
      lastUpdateAt: Date.now() - 7_200_000,
      nextAction: 'Reply to confirm an interview time',
      interviewAt: null,
      interviewTimezone: null,
    },
  ],
  needsReview: [
    {
      provider: 'google',
      messageId: 'a1',
      subject: 'Quick question',
      sender: 'Globex <talent@globex.com>',
      receivedAt: Date.now() - 600_000,
      category: 'rejection',
      confidence: 0.3,
      webLink: 'https://mail.google.com/mail/#all/a1',
      status: 'ambiguous',
    },
  ],
};

function interview(patch: Partial<InterviewView>): InterviewView {
  return {
    id: 3,
    state: 'confirmed',
    interviewType: 'Technical interview',
    startAt: start,
    endAt: start + 3_600_000,
    timezone: 'Europe/Vienna',
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
        change: 'Application changed to Interview',
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
  it('summarizes applications and lists emails that need review', async () => {
    const navigate = show({ page: 'applications' });
    expect(await screen.findByText('Globex')).toBeTruthy();
    expect(screen.getByText('Interviews scheduled')).toBeTruthy();
    expect(screen.getByText('Reply to confirm an interview time')).toBeTruthy();
    expect(screen.getByText('Quick question')).toBeTruthy();
    expect(screen.getByText(/Confidence 30%/)).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Globex' }));
    expect(navigate).toHaveBeenCalledWith({ page: 'applications', applicationId: 7 });
  });

  it('shows where each change came from', async () => {
    mocks.getApplication.mockResolvedValue(detail(interview({})));
    show({ page: 'applications', applicationId: 7 });
    expect(await screen.findByText('Application changed to Interview')).toBeTruthy();
    expect(screen.getByText(/Source: Gmail message · Confidence: 98%/)).toBeTruthy();
  });

  it('asks before adding a confirmed interview, and can decline', async () => {
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
