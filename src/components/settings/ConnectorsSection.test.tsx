// @vitest-environment jsdom
import '../../test/dom';

import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { NavigationContext, type View } from '../../app/navigation';
import type { ConnectorStatus, ConnectorsOverview } from '../../services/connectorService';
import type { ScheduledTask } from '../../services/taskService';
import { ConnectorsSection as Section } from './ConnectorsSection';

const mocks = vi.hoisted(() => ({
  getConnectors: vi.fn(),
  connectConnector: vi.fn(),
  cancelConnectorSignIn: vi.fn(),
  disconnectConnector: vi.fn(),
  setBackgroundSettings: vi.fn(),
}));
const tasks = vi.hoisted(() => ({ listTasks: vi.fn() }));
const navigate = vi.fn<(view: View) => void>();

vi.mock('../../services/connectorService', () => mocks);
vi.mock('../../services/taskService', () => tasks);
vi.mock('../../services/systemService', () => ({
  openExternalUrl: vi.fn(() => Promise.resolve(null)),
  getSystemTimezone: vi.fn(() => Promise.resolve('Europe/Vienna')),
}));

function ConnectorsSection() {
  return (
    <NavigationContext value={{ view: { page: 'settings' }, navigate }}>
      <Section />
    </NavigationContext>
  );
}

const base = {
  enabled: false,
  accountEmail: null,
  accountName: null,
  lastSyncStartedAt: null,
  lastSyncAt: null,
  message: null,
  detail: null,
};

function card(patch: Partial<ConnectorStatus> & Pick<ConnectorStatus, 'id'>): ConnectorStatus {
  const google = patch.id === 'gmail' || patch.id === 'google_calendar';
  const mail = patch.id === 'gmail' || patch.id === 'outlook_mail';
  return {
    ...base,
    provider: google ? 'google' : 'microsoft',
    kind: mail ? 'mail' : 'calendar',
    name: { gmail: 'Gmail', google_calendar: 'Google Calendar', outlook_mail: 'Outlook Mail', outlook_calendar: 'Outlook Calendar' }[
      patch.id
    ],
    publisher: google ? 'Google' : 'Microsoft',
    description: mail
      ? `Read job-related ${google ? '' : 'Outlook '}emails and track application updates.`.replace('  ', ' ')
      : 'Check availability and manage confirmed interviews.',
    state: 'disconnected',
    permissions: mail
      ? [{ capability: 'mail_read', label: 'Mail — Read', granted: false }]
      : [
          { capability: 'calendar_read', label: 'Calendar — Read events', granted: false },
          { capability: 'calendar_write', label: 'Calendar — Create and update events', granted: false },
        ],
    ...patch,
  } as ConnectorStatus;
}

function overview(connectors: ConnectorStatus[]): ConnectorsOverview {
  return {
    connectors,
    background: { runInBackground: false, startAtLogin: false, trayAvailable: true },
  };
}

const disconnected = overview([
  card({ id: 'gmail' }),
  card({ id: 'google_calendar' }),
  card({ id: 'outlook_mail' }),
  card({ id: 'outlook_calendar' }),
]);

const gmailConnected = card({
  id: 'gmail',
  state: 'connected',
  enabled: true,
  accountEmail: 'ana@gmail.com',
  lastSyncAt: Date.now() - 5 * 60_000,
  permissions: [{ capability: 'mail_read', label: 'Mail — Read', granted: true }],
});

beforeEach(() => {
  vi.clearAllMocks();
  mocks.getConnectors.mockResolvedValue(disconnected);
  tasks.listTasks.mockResolvedValue([]);
});

describe('Settings → Connectors', () => {
  it('shows exactly four connectors with their publisher and description, and no OAuth fields', async () => {
    render(<ConnectorsSection />);
    expect(await screen.findByText('Gmail')).toBeTruthy();
    for (const name of ['Google Calendar', 'Outlook Mail', 'Outlook Calendar']) {
      expect(screen.getByText(name)).toBeTruthy();
    }
    expect(screen.getAllByText('by Google')).toHaveLength(2);
    expect(screen.getAllByText('by Microsoft')).toHaveLength(2);
    expect(screen.getByText('Read job-related emails and track application updates.')).toBeTruthy();
    expect(screen.getByText('Read job-related Outlook emails and track application updates.')).toBeTruthy();
    expect(screen.getAllByText('Check availability and manage confirmed interviews.')).toHaveLength(2);
    expect(screen.getAllByRole('button', { name: /^Add / })).toHaveLength(4);
    expect(screen.queryByText(/client (id|secret)/i)).toBeNull();
    expect(screen.queryByRole('textbox')).toBeNull();
  });

  it('connects with + and shows the connected check', async () => {
    mocks.connectConnector.mockResolvedValue(overview([gmailConnected]));
    render(<ConnectorsSection />);
    fireEvent.click(await screen.findByRole('button', { name: 'Add Gmail' }));
    await waitFor(() => expect(mocks.connectConnector).toHaveBeenCalledWith('gmail'));

    mocks.getConnectors.mockResolvedValue(overview([gmailConnected]));
    render(<ConnectorsSection />);
    expect(await screen.findByTitle('Connected')).toBeTruthy();
    expect(screen.getByText('ana@gmail.com')).toBeTruthy();
    expect(screen.getByText('Synced 5 min ago')).toBeTruthy();
  });

  it('shows the sign-in in progress with a way to cancel', async () => {
    mocks.getConnectors.mockResolvedValue(
      overview([card({ id: 'outlook_mail', state: 'connecting', message: 'Finish signing in with Microsoft in your browser.' })]),
    );
    mocks.cancelConnectorSignIn.mockResolvedValue(null);
    render(<ConnectorsSection />);
    expect(await screen.findByText('Finish signing in with Microsoft in your browser.')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(mocks.cancelConnectorSignIn).toHaveBeenCalledWith('microsoft');
  });

  it('asks to reconnect after access was revoked, with technical details on demand', async () => {
    mocks.getConnectors.mockResolvedValue(
      overview([
        card({
          id: 'gmail',
          state: 'reauth_required',
          enabled: true,
          message: 'Google access was revoked or has expired. Reconnect to continue.',
          detail: 'Token has been expired or revoked.',
        }),
      ]),
    );
    render(<ConnectorsSection />);
    expect(await screen.findByText('Google access was revoked or has expired. Reconnect to continue.')).toBeTruthy();
    expect(screen.queryByText('Token has been expired or revoked.')).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Show details' }));
    expect(screen.getByText('Token has been expired or revoked.')).toBeTruthy();
    mocks.connectConnector.mockResolvedValue(disconnected);
    fireEvent.click(screen.getByRole('button', { name: 'Reconnect' }));
    expect(mocks.connectConnector).toHaveBeenCalledWith('gmail');
  });

  it('opens details and disconnects only after confirming', async () => {
    mocks.getConnectors.mockResolvedValue(overview([gmailConnected, card({ id: 'google_calendar' })]));
    mocks.disconnectConnector.mockResolvedValue(disconnected);
    render(<ConnectorsSection />);
    fireEvent.click(await screen.findByRole('button', { name: 'Gmail details' }));
    const detail = screen.getByRole('dialog', { name: 'Gmail' });
    expect(within(detail).getByText('Mail — Read')).toBeTruthy();
    // No sync controls of its own: only the built-in task reads mail.
    expect(within(detail).queryByRole('switch')).toBeNull();
    expect(within(detail).queryByRole('button', { name: 'Sync now' })).toBeNull();
    expect(
      await within(detail).findByText(/Not read automatically: turn on Job Mail & Interview Sync in Scheduled Tasks/),
    ).toBeTruthy();
    fireEvent.click(within(detail).getByRole('button', { name: 'Disconnect' }));

    const confirm = screen.getByRole('dialog', { name: 'Disconnect Gmail?' });
    expect(within(confirm).getByText(/Your tracked applications and their history stay in ReMa/)).toBeTruthy();
    expect(within(confirm).getByText(/revokes its access at Google/)).toBeTruthy();
    expect(mocks.disconnectConnector).not.toHaveBeenCalled();
    fireEvent.click(within(confirm).getByRole('button', { name: 'Disconnect' }));
    await waitFor(() => expect(mocks.disconnectConnector).toHaveBeenCalledWith('gmail'));
  });

  it('says that only Job Mail & Interview Sync reads mail', async () => {
    render(<ConnectorsSection />);
    expect(await screen.findByText(/Connecting a mailbox never starts reading it/)).toBeTruthy();
    expect(await screen.findByText('Off')).toBeTruthy();
    expect(screen.queryByRole('radio')).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Scheduled Tasks' }));
    expect(navigate).toHaveBeenCalledWith({ page: 'tasks' });
  });

  it('shows tracking as on when the built-in task is on', async () => {
    tasks.listTasks.mockResolvedValue([{ id: 1, builtin: 'job_mail_sync', enabled: true } as ScheduledTask]);
    render(<ConnectorsSection />);
    expect(await screen.findByText('On')).toBeTruthy();
  });

  it('offers explicit background options, off by default', async () => {
    mocks.setBackgroundSettings.mockResolvedValue(disconnected);
    render(<ConnectorsSection />);
    const run = (await screen.findByRole('switch', { name: 'Run ReMa in background' })) as HTMLInputElement;
    const login = screen.getByRole('switch', { name: 'Start ReMa at login' }) as HTMLInputElement;
    expect(run.checked).toBe(false);
    expect(login.checked).toBe(false);
    fireEvent.click(login);
    await waitFor(() => expect(mocks.setBackgroundSettings).toHaveBeenCalledWith(false, true));
  });
});
