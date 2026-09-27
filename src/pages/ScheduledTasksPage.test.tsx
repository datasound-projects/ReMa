// @vitest-environment jsdom
import '../test/dom';

import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { useState } from 'react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { NavigationContext, type View } from '../app/navigation';
import type { ScheduledTask, TaskRun, TaskRunSummary } from '../services/taskService';
import { ScheduledTasksPage } from './ScheduledTasksPage';

const mocks = vi.hoisted(() => ({
  listTasks: vi.fn(),
  createTask: vi.fn(),
  updateTask: vi.fn(),
  setTaskEnabled: vi.fn(),
  deleteTask: vi.fn(),
  runTaskNow: vi.fn(),
  listTaskRuns: vi.fn(),
  getTaskRun: vi.fn(),
  cancelTaskRun: vi.fn(),
}));
vi.mock('../services/taskService', () => mocks);

// Backend events, delivered by hand.
const events = vi.hoisted(() => {
  const handlers = new Map<string, Set<(payload: unknown) => void>>();
  const cache = new Map<string, { name: string }>();
  return {
    handlers,
    backendEvents: new Proxy(
      {},
      {
        get: (_target, name) => {
          const key = String(name);
          if (!cache.has(key)) cache.set(key, { name: key });
          return cache.get(key);
        },
      },
    ),
    subscribe: (event: { name: string }, handler: (payload: unknown) => void) => {
      const set = handlers.get(event.name) ?? new Set();
      set.add(handler);
      handlers.set(event.name, set);
      return () => set.delete(handler);
    },
  };
});
vi.mock('../services/events', () => ({ backendEvents: events.backendEvents, subscribe: events.subscribe }));
function emit(name: string, payload: unknown) {
  act(() => events.handlers.get(name)?.forEach((handler) => handler(payload)));
}
vi.mock('../services/analyticsService', () => ({
  listTaskJobRuns: vi.fn(() => Promise.resolve([])),
  analyzeTaskResult: vi.fn(),
}));

/** The page inside a working navigation (row clicks open the task). */
function Harness({ start = { page: 'tasks' } }: { start?: View }) {
  const [view, setView] = useState<View>(start);
  return (
    <NavigationContext value={{ view, navigate: setView }}>
      {view.page === 'tasks' ? (
        <ScheduledTasksPage taskId={view.taskId ?? null} runId={view.runId ?? null} />
      ) : (
        <p>{`Now on ${view.page}${view.page === 'settings' && view.focus ? ` (${view.focus})` : ''}`}</p>
      )}
    </NavigationContext>
  );
}
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

const HOUR = 3_600_000;
const now = Date.now();

function summary(id: number, trigger: 'scheduled' | 'manual' = 'scheduled'): TaskRunSummary {
  const at = now - (100 - id) * HOUR;
  return {
    id,
    taskId: 1,
    trigger,
    status: 'succeeded',
    scheduledFor: trigger === 'scheduled' ? at : null,
    queuedAt: at,
    startedAt: at + 1_000,
    finishedAt: at + 49_000,
    errorCategory: null,
  };
}

function stage(id: number, key: string, label: string, status: TaskRun['progress'][number]['status']) {
  return { id, stage: key, label, status, startedAt: now, updatedAt: now };
}

function full(id: number, over: Partial<TaskRun> & { trigger?: 'scheduled' | 'manual' } = {}): TaskRun {
  const base = summary(id, over.trigger ?? 'scheduled');
  return {
    ...base,
    durationMs: 48_000,
    taskName: 'Vienna AI jobs',
    kind: { type: 'prompt' },
    prompt: 'Find AI engineering jobs in Vienna',
    model: { providerId: 'anthropic', modelId: 'model-a' },
    schedule: { schedule: { kind: 'daily', every: 1 }, timezone: 'Europe/Vienna', startDate: '2026-09-20', startTime: '21:30' },
    useProfile: false,
    context: { profile: false, connectors: [], webSearch: true, searches: 2, searchEngines: ['ChatGPT web search'] },
    result: 'ok',
    report: null,
    error: null,
    progress: [stage(1, 'answer', 'Answer received', 'completed')],
    outputs: [],
    ...over,
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  mocks.listTasks.mockResolvedValue([prompt]);
});

describe('Scheduled Tasks', () => {
  it('offers Job Mail & Interview Sync as a built-in task that is off until set up', async () => {
    render(<Harness />);
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
    render(<Harness />);
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
    render(<Harness />);
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
    render(<Harness />);
    const card = await screen.findByRole('article', { name: 'Job Mail & Interview Sync' });
    expect((within(card).getByRole('button', { name: 'Run now' }) as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(within(card).getByRole('button', { name: 'Task actions' }));
    const item = await screen.findByRole('menuitem', { name: 'Run now' });
    expect(item.getAttribute('aria-disabled') === 'true' || (item as HTMLButtonElement).disabled).toBe(true);
  });

  it('opens a task into its runs, newest first, each with its own result', async () => {
    mocks.listTaskRuns.mockResolvedValue({ runs: [summary(3, 'manual'), summary(2), summary(1)], hasMore: false });
    mocks.getTaskRun.mockImplementation((id: number) =>
      Promise.resolve(
        id === 1
          ? full(1, { result: 'Monday: **9 roles**', prompt: 'Search Vienna', taskName: 'Vienna jobs' })
          : full(id, { result: id === 3 ? 'Tuesday: 14 roles' : 'Sunday: 4 roles', trigger: id === 3 ? 'manual' : 'scheduled' }),
      ),
    );
    render(<Harness />);
    fireEvent.click(await screen.findByText('Vienna AI jobs'));

    // "Scheduled / Vienna AI jobs"
    expect(await screen.findByRole('heading', { level: 1, name: 'Vienna AI jobs' })).toBeTruthy();
    const breadcrumb = screen.getByRole('navigation', { name: 'Breadcrumb' });
    expect(within(breadcrumb).getByRole('button', { name: 'Scheduled' })).toBeTruthy();

    const history = screen.getByRole('complementary', { name: 'Run history' });
    const rows = within(history).getAllByRole('button', { name: /^Run — / });
    expect(rows).toHaveLength(3);
    const [newest, , oldest] = rows;
    if (!newest || !oldest) throw new Error('three runs expected');
    expect(newest.getAttribute('aria-current')).toBe('true');
    expect(within(newest).getByText('Manual')).toBeTruthy();
    expect(await screen.findByText('Tuesday: 14 roles')).toBeTruthy();

    fireEvent.click(oldest);
    expect(await screen.findByText('9 roles')).toBeTruthy();
    expect(screen.queryByText('Tuesday: 14 roles')).toBeNull();
    expect(mocks.getTaskRun).toHaveBeenLastCalledWith(1);
    // The task as it was then.
    fireEvent.click(within(history).getByRole('button', { name: 'Task configuration' }));
    expect(within(history).getByText('Search Vienna')).toBeTruthy();
    expect(within(history).getByText('Vienna jobs')).toBeTruthy();
    fireEvent.click(within(history).getByRole('button', { name: 'Context' }));
    expect(within(history).getByText('Enabled · ChatGPT web search · 2 searches')).toBeTruthy();
  });

  it('shows a failed run with its error and the way to fix it', async () => {
    mocks.listTaskRuns.mockResolvedValue({ runs: [{ ...summary(5), status: 'failed', errorCategory: 'connector' }], hasMore: false });
    mocks.getTaskRun.mockResolvedValue(
      full(5, {
        status: 'failed',
        result: null,
        errorCategory: 'connector',
        error: 'Gmail access was revoked or has expired. Reconnect in Settings → Connectors.',
      }),
    );
    render(<Harness start={{ page: 'tasks', taskId: 1 }} />);
    const alert = await screen.findByRole('alert');
    expect(within(alert).getByText(/Gmail access was revoked/)).toBeTruthy();
    expect(within(alert).getByText(/A connected service needs attention/)).toBeTruthy();
    fireEvent.click(within(alert).getByRole('button', { name: 'Open Connectors' }));
    expect(await screen.findByText('Now on settings (connectors)')).toBeTruthy();
  });

  it('follows a running run and shows its result when it finishes', async () => {
    mocks.listTaskRuns.mockResolvedValue({ runs: [{ ...summary(8), status: 'running' }], hasMore: false });
    mocks.getTaskRun.mockResolvedValue(
      full(8, {
        status: 'running',
        result: null,
        finishedAt: null,
        progress: [
          stage(1, 'profile', 'Loaded your Profile', 'completed'),
          stage(2, 'answer', 'Waiting for the model\'s answer', 'running'),
        ],
      }),
    );
    render(<Harness start={{ page: 'tasks', taskId: 1 }} />);
    expect(await screen.findByText('The result appears here when the run finishes.')).toBeTruthy();
    const main = screen.getByRole('article');
    expect(within(main).getByText('Running')).toBeTruthy();
    expect(within(main).getByText('Loaded your Profile')).toBeTruthy();
    expect(within(main).getByText("Waiting for the model's answer")).toBeTruthy();

    mocks.getTaskRun.mockResolvedValue(full(8, { result: 'Done: 3 roles' }));
    mocks.listTaskRuns.mockResolvedValue({ runs: [summary(8)], hasMore: false });
    emit('taskRunChanged', { taskId: 1, runId: 8 });
    expect(await screen.findByText('Done: 3 roles')).toBeTruthy();
    expect(within(screen.getByRole('article')).getByText('Succeeded')).toBeTruthy();
  });

  it('opens the new run after Run now', async () => {
    mocks.runTaskNow.mockResolvedValue(42);
    mocks.listTaskRuns.mockResolvedValue({ runs: [{ ...summary(42, 'manual'), status: 'queued' }], hasMore: false });
    mocks.getTaskRun.mockResolvedValue(full(42, { status: 'queued', trigger: 'manual', startedAt: null, finishedAt: null, result: null }));
    render(<Harness />);
    await screen.findByText('Vienna AI jobs');
    fireEvent.click(screen.getByRole('button', { name: 'Task actions' }));
    fireEvent.click(await screen.findByRole('menuitem', { name: 'Run now' }));
    expect(await screen.findByRole('heading', { level: 1, name: 'Vienna AI jobs' })).toBeTruthy();
    await waitFor(() => expect(mocks.getTaskRun).toHaveBeenCalledWith(42));
    expect(within(await screen.findByRole('article')).getByText('Queued')).toBeTruthy();
  });

  it('loads older runs only when asked', async () => {
    const newest = Array.from({ length: 30 }, (_, i) => summary(100 - i));
    mocks.listTaskRuns.mockImplementation((_task: number, before: number | null) =>
      Promise.resolve(
        before === null
          ? { runs: newest, hasMore: true }
          : { runs: [summary(70), summary(69)], hasMore: false },
      ),
    );
    mocks.getTaskRun.mockImplementation((id: number) => Promise.resolve(full(id)));
    render(<Harness start={{ page: 'tasks', taskId: 1 }} />);
    const history = await screen.findByRole('complementary', { name: 'Run history' });
    await waitFor(() => expect(within(history).getAllByRole('button', { name: /^Run — / })).toHaveLength(30));
    fireEvent.click(within(history).getByRole('button', { name: 'Load older runs' }));
    await waitFor(() => expect(within(history).getAllByRole('button', { name: /^Run — / })).toHaveLength(32));
    expect(mocks.listTaskRuns).toHaveBeenLastCalledWith(1, 71);
    expect(within(history).queryByRole('button', { name: 'Load older runs' })).toBeNull();
  });

  it('explains a task that has not run yet', async () => {
    mocks.listTaskRuns.mockResolvedValue({ runs: [], hasMore: false });
    render(<Harness start={{ page: 'tasks', taskId: 1 }} />);
    expect(
      await screen.findByText('This task has not run yet. Its execution history and results will appear here.'),
    ).toBeTruthy();
    expect(screen.getByText('No runs yet', { selector: '.empty-state__title' })).toBeTruthy();
    expect(screen.getAllByRole('button', { name: 'Run now' }).length).toBeGreaterThan(0);
  });
});
