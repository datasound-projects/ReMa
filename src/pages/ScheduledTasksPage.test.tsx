// @vitest-environment jsdom
import '../test/dom';

import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { ScheduledTask } from '../services/taskService';
import { ScheduledTasksPage } from './ScheduledTasksPage';

const mocks = vi.hoisted(() => ({
  listTasks: vi.fn(),
  createTask: vi.fn(),
  updateTask: vi.fn(),
  setTaskEnabled: vi.fn(),
  deleteTask: vi.fn(),
  runTaskNow: vi.fn(),
  listTaskExecutions: vi.fn(),
}));
vi.mock('../services/taskService', () => mocks);
vi.mock('../services/providerService', () => ({
  getModelCatalog: vi.fn(() =>
    Promise.resolve({
      models: [{ model: { providerId: 'anthropic', modelId: 'model-a' }, displayName: 'Model A', providerName: 'Anthropic' }],
      defaultModel: { providerId: 'anthropic', modelId: 'model-a' },
    }),
  ),
  sameModel: (a: { modelId: string }, b: { modelId: string }) => a.modelId === b.modelId,
}));
vi.mock('../services/systemService', () => ({ getSystemTimezone: vi.fn(() => Promise.resolve('Europe/Vienna')) }));
vi.mock('../services/connectorService', () => ({
  getConnectors: vi.fn(() =>
    Promise.resolve({
      connectors: [
        { id: 'gmail', kind: 'mail', name: 'Gmail', state: 'connected', accountEmail: 'ana@gmail.com' },
        { id: 'google_calendar', kind: 'calendar', name: 'Google Calendar', state: 'connected', accountEmail: null },
      ],
      background: { runInBackground: false, startAtLogin: false, trayAvailable: true },
    }),
  ),
}));

const prompt = {
  id: 1,
  name: 'Vienna AI jobs',
  kind: { type: 'prompt' },
  builtin: null,
  prompt: 'Find AI engineering jobs in Vienna',
  useProfile: false,
  model: { providerId: 'anthropic', modelId: 'model-a' },
  schedule: { kind: 'daily', every: 1 },
  timezone: 'Europe/Vienna',
  startDate: '2026-09-28',
  startTime: '08:00',
  startAt: 0,
  end: { kind: 'never' },
  endAt: null,
  maxRuns: null,
  runCount: 0,
  enabled: true,
  status: 'active',
  running: false,
  lastRunAt: null,
  lastRunStatus: null,
  nextRunAt: null,
  createdAt: 0,
  updatedAt: 0,
} as unknown as ScheduledTask;

beforeEach(() => {
  vi.clearAllMocks();
  mocks.listTasks.mockResolvedValue([prompt]);
});

describe('Scheduled Tasks', () => {
  it('offers Job Mail & Interview Sync as a built-in task that is off until set up', async () => {
    render(<ScheduledTasksPage />);
    const card = await screen.findByRole('article', { name: 'Job Mail & Interview Sync' });
    expect(
      within(card).getByText(
        'Tracks job-application emails, updates Applications, detects actions and confirmed interviews, and syncs confirmed interviews with your calendar.',
      ),
    ).toBeTruthy();
    expect(within(card).getByText(/ReMa does not read your mail until you set this up/)).toBeTruthy();
    // The user's own tasks are listed separately and keep working.
    expect(screen.getByText('Vienna AI jobs')).toBeTruthy();

    mocks.createTask.mockResolvedValue({ ...prompt, id: 2, builtin: 'job_mail_sync' });
    fireEvent.click(within(card).getByRole('button', { name: 'Set up' }));
    const dialog = await screen.findByRole('dialog', { name: 'Set up Job Mail & Interview Sync' });
    expect(within(dialog).queryByLabelText('Name')).toBeNull();
    expect(await within(dialog).findByText('Gmail')).toBeTruthy();
    const lookback = within(dialog).getByLabelText('Initial lookback: last') as HTMLInputElement;
    expect(lookback.value).toBe('30');
    fireEvent.change(lookback, { target: { value: '60' } });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Turn on' }));
    await waitFor(() =>
      expect(mocks.createTask).toHaveBeenCalledWith(
        expect.objectContaining({
          kind: { type: 'job_applications', lookbackDays: 60, syncCalendar: true },
          schedule: { kind: 'interval', every: 1, unit: 'hours' },
          useProfile: false,
        }),
      ),
    );
  });

  it('turns the built-in task on and off with the existing scheduler', async () => {
    mocks.listTasks.mockResolvedValue([
      prompt,
      {
        ...prompt,
        id: 2,
        name: 'Job Mail & Interview Sync',
        builtin: 'job_mail_sync',
        kind: { type: 'job_applications', lookbackDays: 30, syncCalendar: true },
        schedule: { kind: 'interval', every: 1, unit: 'hours' },
      },
    ]);
    mocks.setTaskEnabled.mockResolvedValue(prompt);
    render(<ScheduledTasksPage />);
    const card = await screen.findByRole('article', { name: 'Job Mail & Interview Sync' });
    expect(within(card).getByText(/^last 30 days · Calendar · /)).toBeTruthy();
    fireEvent.click(within(card).getByRole('switch', { name: 'Job Mail & Interview Sync' }));
    await waitFor(() => expect(mocks.setTaskEnabled).toHaveBeenCalledWith(2, false));
    // Listed once, in its card, not among the user's own tasks.
    expect(screen.getAllByText('Job Mail & Interview Sync', { selector: 'h3, .task-table__title' })).toHaveLength(1);
  });

  it('keeps an old mail task paused and says why', async () => {
    mocks.listTasks.mockResolvedValue([
      {
        ...prompt,
        id: 7,
        name: 'Old mail monitor',
        builtin: null,
        enabled: false,
        kind: { type: 'job_applications', lookbackDays: 7, syncCalendar: false },
      },
    ]);
    render(<ScheduledTasksPage />);
    expect(await screen.findByText(/No longer runs: only Job Mail & Interview Sync reads mail/)).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Task actions' }));
    for (const name of ['Run now', 'Resume']) {
      const item = await screen.findByRole('menuitem', { name });
      expect(item.getAttribute('aria-disabled') === 'true' || (item as HTMLButtonElement).disabled).toBe(true);
    }
  });

  it('offers no way to read mail while the built-in task is off', async () => {
    mocks.listTasks.mockResolvedValue([
      {
        ...prompt,
        id: 2,
        name: 'Job Mail & Interview Sync',
        builtin: 'job_mail_sync',
        enabled: false,
        kind: { type: 'job_applications', lookbackDays: 30, syncCalendar: true },
        schedule: { kind: 'interval', every: 1, unit: 'hours' },
      },
    ]);
    render(<ScheduledTasksPage />);
    const card = await screen.findByRole('article', { name: 'Job Mail & Interview Sync' });
    expect((within(card).getByRole('button', { name: 'Run now' }) as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(within(card).getByRole('button', { name: 'Task actions' }));
    const item = await screen.findByRole('menuitem', { name: 'Run now' });
    expect(item.getAttribute('aria-disabled') === 'true' || (item as HTMLButtonElement).disabled).toBe(true);
  });
});
