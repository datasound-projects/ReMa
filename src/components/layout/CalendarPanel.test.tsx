// @vitest-environment jsdom
import '../../test/dom';

import { fireEvent, render, screen, within } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { NavigationContext, type View } from '../../app/navigation';
import type { CalendarEntry, CalendarView } from '../../services/applicationService';
import { CalendarButton } from './CalendarPanel';

const mocks = vi.hoisted(() => ({ getCalendar: vi.fn(), openExternalUrl: vi.fn() }));
vi.mock('../../services/applicationService', () => ({ getCalendar: mocks.getCalendar }));
vi.mock('../../services/systemService', () => ({ openExternalUrl: mocks.openExternalUrl }));

const HOUR = 3_600_000;
const today = new Date();
today.setHours(0, 0, 0, 0);
const at = (hours: number) => today.getTime() + hours * HOUR;

function entry(patch: Partial<CalendarEntry> & Pick<CalendarEntry, 'key' | 'title'>): CalendarEntry {
  return {
    provider: 'google',
    startAt: at(9),
    endAt: at(10),
    allDay: false,
    location: null,
    meetingUrl: null,
    webLink: null,
    interview: null,
    ...patch,
  };
}

const both: CalendarView = {
  calendars: [
    { connector: 'google_calendar', provider: 'google', name: 'Google Calendar', accountEmail: 'ana@gmail.com', ready: true },
    { connector: 'outlook_calendar', provider: 'microsoft', name: 'Outlook Calendar', accountEmail: null, ready: true },
  ],
  entries: [
    entry({ key: 'google:1', title: 'Standup' }),
    entry({ key: 'microsoft:2', title: 'Dentist', provider: 'microsoft', startAt: at(11), endAt: at(12) }),
    entry({
      key: 'google:3',
      title: 'Interview — Anthropic — Forward Deployed Engineer',
      startAt: at(14),
      endAt: at(15),
      meetingUrl: 'https://meet.google.com/abc-defg-hij',
      interview: {
        applicationId: 10,
        interviewId: 1,
        company: 'Anthropic',
        role: 'Forward Deployed Engineer',
        inCalendar: true,
        conflict: false,
        cancelled: false,
      },
    }),
    entry({
      key: 'interview:2',
      title: 'Interview — Globex — Data Engineer',
      provider: null,
      startAt: at(16),
      endAt: at(17),
      interview: {
        applicationId: 11,
        interviewId: 2,
        company: 'Globex',
        role: 'Data Engineer',
        inCalendar: false,
        conflict: true,
        cancelled: false,
      },
    }),
  ],
  problems: [],
};

const navigate = vi.fn<(view: View) => void>();

function open() {
  render(
    <NavigationContext value={{ view: { page: 'chat', conversationId: null }, navigate }}>
      <CalendarButton />
    </NavigationContext>,
  );
  fireEvent.click(screen.getByRole('button', { name: 'Calendar' }));
  return screen.getByRole('dialog', { name: 'Calendar' });
}

beforeEach(() => {
  vi.clearAllMocks();
  mocks.openExternalUrl.mockResolvedValue(null);
});

describe('In-app calendar', () => {
  it('says how to connect a calendar when none is connected', async () => {
    mocks.getCalendar.mockResolvedValue({ calendars: [], entries: [], problems: [] });
    const panel = open();
    expect(await within(panel).findByText('No calendar connected.')).toBeTruthy();
    expect(within(panel).getByText('Connect Google Calendar or Outlook Calendar in Settings → Connectors.')).toBeTruthy();
    fireEvent.click(within(panel).getByRole('button', { name: 'Open Connectors' }));
    expect(navigate).toHaveBeenCalledWith({ page: 'settings', focus: 'connectors' });
  });

  it('shows both calendars with interviews marked and meeting links one click away', async () => {
    mocks.getCalendar.mockResolvedValue(both);
    const panel = open();
    expect(await within(panel).findByText('Standup')).toBeTruthy();
    expect(within(panel).getByText('Dentist')).toBeTruthy();
    expect(within(panel).getByText('Outlook')).toBeTruthy();
    // A week from today, read live from the providers.
    const [start, end] = mocks.getCalendar.mock.calls[0] as [number, number];
    expect(start).toBe(today.getTime());
    expect(Math.round((end - start) / (24 * HOUR))).toBe(7);

    const interview = within(panel).getByText('Interview — Anthropic — Forward Deployed Engineer').closest('li')!;
    expect(interview.className).toContain('calendar-event--interview');
    fireEvent.click(within(interview).getByRole('button', { name: 'Join' }));
    expect(mocks.openExternalUrl).toHaveBeenCalledWith('https://meet.google.com/abc-defg-hij');
    expect(within(panel).getByText('Calendar conflict: not added')).toBeTruthy();

    fireEvent.click(within(interview).getByRole('button', { name: /Anthropic — Forward Deployed Engineer/ }));
    expect(navigate).toHaveBeenCalledWith({ page: 'applications', applicationId: 10 });
    // The title already names the interview; the link does not repeat it.
    expect(within(interview).getByText('View application')).toBeTruthy();
  });

  it('names the application of an interview event the user renamed', async () => {
    const renamed = entry({
      key: 'google:9',
      title: 'Call',
      interview: {
        applicationId: 12,
        interviewId: 3,
        company: 'Initech',
        role: 'QA Engineer',
        inCalendar: true,
        conflict: false,
        cancelled: false,
      },
    });
    mocks.getCalendar.mockResolvedValue({ ...both, entries: [renamed] });
    const panel = open();
    fireEvent.click(await within(panel).findByRole('button', { name: 'Interview · Initech — QA Engineer' }));
    expect(navigate).toHaveBeenCalledWith({ page: 'applications', applicationId: 12 });
  });

  it('moves a week at a time', async () => {
    mocks.getCalendar.mockResolvedValue(both);
    const panel = open();
    await within(panel).findByText('Standup');
    fireEvent.click(within(panel).getByRole('button', { name: 'Next week' }));
    // Today's events are not in next week.
    expect(await within(panel).findByText('No events in these 7 days.')).toBeTruthy();
    const [start] = mocks.getCalendar.mock.calls.at(-1) as [number, number];
    expect(Math.round((start - today.getTime()) / (24 * HOUR))).toBe(7);
  });
});
