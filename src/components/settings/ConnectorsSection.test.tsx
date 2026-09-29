// @vitest-environment jsdom
import '../../test/dom';

import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { NavigationContext, type View } from '../../app/navigation';
import type {
  ConnectorStatus,
  ConnectorsOverview,
  ProviderAccount,
  ProviderId,
} from '../../services/connectorService';
import type { ScheduledTask } from '../../services/taskService';
import { ConnectorsSection as Section } from './ConnectorsSection';
import { testingNote } from '../../lib/connectorNotes';

const mocks = vi.hoisted(() => ({
  getConnectors: vi.fn(),
  connectConnector: vi.fn(),
  connectProviderAccount: vi.fn(),
  cancelConnectorSignIn: vi.fn(),
  disconnectConnector: vi.fn(),
  disconnectProviderAccount: vi.fn(),
  setBackgroundSettings: vi.fn(),
  setConnectionPreferences: vi.fn(),
}));
const tasks = vi.hoisted(() => ({ listTasks: vi.fn(), runTaskNow: vi.fn() }));
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
  errorCode: null,
  signInEndsAt: null,
};

function card(patch: Partial<ConnectorStatus> & Pick<ConnectorStatus, 'id'>): ConnectorStatus {
  const google = patch.id === 'gmail' || patch.id === 'google_calendar';
  const mail = patch.id === 'gmail' || patch.id === 'outlook_mail';
  return {
    ...base,
    provider: google ? 'google' : 'microsoft',
    kind: mail ? 'mail' : 'calendar',
    name: {
      gmail: 'Gmail',
      google_calendar: 'Google Calendar',
      outlook_mail: 'Outlook Mail',
      outlook_calendar: 'Outlook Calendar',
      linkedin: 'LinkedIn',
      xing: 'XING',
    }[patch.id],
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

const added = (c: ConnectorStatus) => c.enabled && c.state !== 'disconnected';

/** The account card the backend derives from a provider's connector cards. */
function account(provider: ProviderId, connectors: ConnectorStatus[], patch: Partial<ProviderAccount> = {}): ProviderAccount {
  const own = connectors.filter((c) => c.provider === provider);
  const connected = own.find((c) => added(c));
  const all = provider === 'google' ? ['gmail', 'google_calendar'] : ['outlook_mail', 'outlook_calendar'];
  return {
    provider,
    name: provider === 'google' ? 'Google' : 'Microsoft',
    state: connected ? 'connected' : 'disconnected',
    connectionId: connected ? `${provider}:acc-1` : null,
    email: connected?.accountEmail ?? null,
    displayName: connected?.accountName ?? null,
    capabilities: all.map((id) => {
      const c = own.find((o) => o.id === id);
      return {
        connector: id as ConnectorStatus['id'],
        name: card({ id: id as ConnectorStatus['id'] }).name,
        granted: c ? added(c) && c.state !== 'permission_missing' && c.state !== 'reauth_required' : false,
        state: c?.state ?? 'disconnected',
      };
    }),
    message: null,
    detail: null,
    errorCode: null,
    connectedAt: connected ? Date.now() - 3_600_000 : null,
    lastRefreshedAt: null,
    signInEndsAt: null,
    available: true,
    ...patch,
  };
}

function overview(
  connectors: ConnectorStatus[],
  patch: Partial<ConnectorsOverview> = {},
  accounts: { google?: Partial<ProviderAccount>; microsoft?: Partial<ProviderAccount> } = {},
): ConnectorsOverview {
  return {
    connectors,
    accounts: [account('google', connectors, accounts.google), account('microsoft', connectors, accounts.microsoft)],
    background: { runInBackground: false, startAtLogin: false, trayAvailable: true },
    mailProcessing: null,
    preferences: { newAccountsInChats: false },
    ...patch,
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
  it('shows one card per account provider with Connect, and no OAuth fields', async () => {
    render(<ConnectorsSection />);
    expect(await screen.findByText('Google')).toBeTruthy();
    expect(screen.getByText('Microsoft')).toBeTruthy();
    expect(screen.getByText('Gmail and Google Calendar: track job mail and manage confirmed interviews.')).toBeTruthy();
    expect(screen.getByText('Outlook Mail and Outlook Calendar: track job mail and manage confirmed interviews.')).toBeTruthy();
    expect(screen.getAllByRole('button', { name: 'Connect' })).toHaveLength(2);
    expect(screen.getAllByText('Not connected')).toHaveLength(2);
    expect(screen.queryByText(/client (id|secret)/i)).toBeNull();
    expect(screen.queryByLabelText(/token|client/i)).toBeNull();
    expect(screen.queryByRole('textbox')).toBeNull();
  });

  it('connects the whole account in one sign-in and then lists its capabilities', async () => {
    mocks.connectProviderAccount.mockResolvedValue(overview([gmailConnected]));
    render(<ConnectorsSection />);
    const [googleConnect] = await screen.findAllByRole('button', { name: 'Connect' });
    fireEvent.click(googleConnect!);
    await waitFor(() => expect(mocks.connectProviderAccount).toHaveBeenCalledWith('google'));

    mocks.getConnectors.mockResolvedValue(overview([gmailConnected, card({ id: 'google_calendar' })]));
    render(<ConnectorsSection />);
    expect(await screen.findByText('ana@gmail.com')).toBeTruthy();
    const capabilities = screen.getByRole('list', { name: 'Google capabilities' });
    expect(within(capabilities).getByText('Gmail').closest('li')?.className).toBe('is-granted');
    expect(within(capabilities).getByText('Google Calendar').closest('li')?.className).toBe('is-missing');
    const googleCard = screen.getByText('ana@gmail.com').closest('.account-card') as HTMLElement;
    expect(within(googleCard).getByText('Connected')).toBeTruthy();
    expect(within(googleCard).getByRole('button', { name: 'Manage' })).toBeTruthy();
    expect(within(googleCard).getByRole('button', { name: 'Disconnect' })).toBeTruthy();
    expect(within(googleCard).queryByRole('button', { name: 'Connect' })).toBeNull();
  });

  it('shows the sign-in in progress with a way to cancel', async () => {
    mocks.getConnectors.mockResolvedValue(
      overview(
        [card({ id: 'outlook_mail', state: 'connecting' })],
        {},
        { microsoft: { state: 'connecting', message: 'Finish signing in with Microsoft in your browser.' } },
      ),
    );
    mocks.cancelConnectorSignIn.mockResolvedValue(null);
    render(<ConnectorsSection />);
    expect(await screen.findByText('Finish signing in with Microsoft in your browser.')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(mocks.cancelConnectorSignIn).toHaveBeenCalledWith('microsoft');
  });

  it('asks to reconnect after access was revoked, with technical details on demand', async () => {
    mocks.getConnectors.mockResolvedValue(
      overview(
        [card({ id: 'gmail', state: 'reauth_required', enabled: true, accountEmail: 'ana@gmail.com' })],
        {},
        {
          google: {
            state: 'reauth_required',
            errorCode: 'REAUTH_REQUIRED',
            message: 'Google access was revoked or has expired. Reconnect to continue.',
            detail: 'Token has been expired or revoked.',
          },
        },
      ),
    );
    render(<ConnectorsSection />);
    expect(await screen.findByText('Google access was revoked or has expired. Reconnect to continue.')).toBeTruthy();
    expect(screen.getByText('Reconnect required')).toBeTruthy();
    expect(screen.queryByText('Token has been expired or revoked.')).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Show details' }));
    expect(screen.getByText('Token has been expired or revoked.')).toBeTruthy();
    mocks.connectProviderAccount.mockResolvedValue(disconnected);
    fireEvent.click(screen.getByRole('button', { name: 'Reconnect' }));
    expect(mocks.connectProviderAccount).toHaveBeenCalledWith('google');
  });

  it('says in advance when Google ends a sign-in while its app is in Testing', async () => {
    const endsAt = new Date(2026, 9, 5, 12).getTime();
    mocks.getConnectors.mockResolvedValue(
      overview([{ ...gmailConnected, signInEndsAt: endsAt }], {}, { google: { signInEndsAt: endsAt } }),
    );
    render(<ConnectorsSection />);
    expect(await screen.findByText(testingNote(endsAt))).toBeTruthy();
    expect(testingNote(endsAt)).toContain('in Testing');
    expect(testingNote(endsAt)).toContain('publishing the app removes the limit');
  });

  it('shows no such date for a working connection otherwise', async () => {
    mocks.getConnectors.mockResolvedValue(overview([gmailConnected]));
    render(<ConnectorsSection />);
    await screen.findByText('ana@gmail.com');
    expect(screen.queryByText(/in Testing/)).toBeNull();
  });

  it('disconnects the account only after confirming', async () => {
    mocks.getConnectors.mockResolvedValue(overview([gmailConnected, card({ id: 'google_calendar' })]));
    mocks.disconnectProviderAccount.mockResolvedValue(disconnected);
    render(<ConnectorsSection />);
    fireEvent.click(await screen.findByRole('button', { name: 'Disconnect' }));
    const confirm = screen.getByRole('dialog', { name: 'Disconnect Google?' });
    expect(within(confirm).getByText(/Your tracked applications and their history stay in ReMa/)).toBeTruthy();
    expect(within(confirm).getByText(/revokes its access at Google/)).toBeTruthy();
    expect(mocks.disconnectProviderAccount).not.toHaveBeenCalled();
    fireEvent.click(within(confirm).getByRole('button', { name: 'Disconnect' }));
    await waitFor(() => expect(mocks.disconnectProviderAccount).toHaveBeenCalledWith('google'));
  });

  it('manages capabilities one by one: add, remove after confirming, permissions, no sync switches', async () => {
    mocks.getConnectors.mockResolvedValue(overview([gmailConnected, card({ id: 'google_calendar' })]));
    mocks.connectConnector.mockResolvedValue(disconnected);
    mocks.disconnectConnector.mockResolvedValue(disconnected);
    render(<ConnectorsSection />);
    fireEvent.click(await screen.findByRole('button', { name: 'Manage' }));
    const detail = screen.getByRole('dialog', { name: 'Google account' });
    expect(within(detail).getByText('Mail — Read')).toBeTruthy();
    expect(within(detail).queryByRole('switch')).toBeNull();
    expect(within(detail).queryByRole('button', { name: 'Sync now' })).toBeNull();
    expect(within(detail).getByText('Access is renewed only when a request needs it.')).toBeTruthy();
    expect(
      await within(detail).findByText(/Not read automatically: turn on Job Mail & Interview Sync in Scheduled Tasks/),
    ).toBeTruthy();

    fireEvent.click(within(detail).getByRole('button', { name: 'Add Google Calendar' }));
    await waitFor(() => expect(mocks.connectConnector).toHaveBeenCalledWith('google_calendar'));

    fireEvent.click(within(detail).getByRole('button', { name: 'Remove Gmail' }));
    const confirm = screen.getByRole('dialog', { name: 'Remove Gmail?' });
    expect(within(confirm).getByText(/last capability of this account/)).toBeTruthy();
    expect(mocks.disconnectConnector).not.toHaveBeenCalled();
    fireEvent.click(within(confirm).getByRole('button', { name: 'Remove' }));
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

  it('keeps new accounts out of chats with their own choice unless asked', async () => {
    mocks.setConnectionPreferences.mockResolvedValue(disconnected);
    render(<ConnectorsSection />);
    const choice = (await screen.findByRole('switch', {
      name: 'Add new accounts to chats that chose their own connectors',
    })) as HTMLInputElement;
    expect(choice.checked).toBe(false);
    expect(screen.getByText(/Turning a connector off in a chat never disconnects the account/)).toBeTruthy();
    fireEvent.click(choice);
    await waitFor(() => expect(mocks.setConnectionPreferences).toHaveBeenCalledWith({ newAccountsInChats: true }));
  });

  it('reports a failed sign-in on the card, with Retry, and names admin approval', async () => {
    mocks.getConnectors.mockResolvedValue(
      overview([card({ id: 'outlook_mail' })], {}, {
        microsoft: {
          state: 'admin_approval_required',
          errorCode: 'PROVIDER_ADMIN_POLICY',
          message:
            'Your organization requires administrator approval before ReMa can access this Microsoft account.',
          detail: 'access_denied: AADSTS90094',
        },
      }),
    );
    mocks.connectProviderAccount.mockResolvedValue(disconnected);
    render(<ConnectorsSection />);
    expect(await screen.findByText('Approval required')).toBeTruthy();
    expect(
      screen.getByText(
        'Your organization requires administrator approval before ReMa can access this Microsoft account.',
      ),
    ).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
    expect(mocks.connectProviderAccount).toHaveBeenCalledWith('microsoft');
    // OAuth internals only on demand.
    expect(screen.queryByText('access_denied: AADSTS90094')).toBeNull();
  });

  it('tells a developer build what public configuration is missing, without a Connect button', async () => {
    mocks.getConnectors.mockResolvedValue(
      overview([card({ id: 'gmail', state: 'unavailable' })], {}, {
        google: {
          state: 'unavailable',
          available: false,
          message: 'Development build without ReMa’s Google public app configuration ([google] desktop_client_id).',
        },
      }),
    );
    render(<ConnectorsSection />);
    expect(await screen.findByText(/\[google\] desktop_client_id/)).toBeTruthy();
    expect(screen.getAllByRole('button', { name: 'Connect' })).toHaveLength(1);
    expect(screen.getByText('Unavailable')).toBeTruthy();
  });

  it('says plainly where job mail is read: a cloud provider or this computer', async () => {
    mocks.getConnectors.mockResolvedValue(
      overview([gmailConnected], {
        mailProcessing: { model: 'claude-sonnet-5', recipient: 'Anthropic', onDevice: false },
      }),
    );
    const { unmount } = render(<ConnectorsSection />);
    expect(
      await screen.findByText(/Job-related email text is sent to Anthropic \(claude-sonnet-5\) to be read/),
    ).toBeTruthy();
    expect(screen.queryByText(/never leaves/)).toBeNull();
    unmount();

    mocks.getConnectors.mockResolvedValue(
      overview([gmailConnected], {
        mailProcessing: { model: 'qwen3-14b', recipient: 'this computer', onDevice: true },
      }),
    );
    render(<ConnectorsSection />);
    expect(
      await screen.findByText(/read by qwen3-14b on this computer: mail leaves it only between ReMa and Google or Microsoft/),
    ).toBeTruthy();
  });

  it('syncs through the built-in task only, from Manage', async () => {
    tasks.listTasks.mockResolvedValue([{ id: 7, builtin: 'job_mail_sync', enabled: true } as ScheduledTask]);
    tasks.runTaskNow.mockResolvedValue(42);
    mocks.getConnectors.mockResolvedValue(overview([gmailConnected, card({ id: 'google_calendar' })]));
    render(<ConnectorsSection />);
    fireEvent.click(await screen.findByRole('button', { name: 'Manage' }));
    const detail = screen.getByRole('dialog', { name: 'Google account' });
    expect(within(detail).getByText('Not added')).toBeTruthy();
    fireEvent.click(await within(detail).findByRole('button', { name: 'Sync now' }));
    await waitFor(() => expect(tasks.runTaskNow).toHaveBeenCalledWith(7));
    expect(await within(detail).findByText(/Job Mail & Interview Sync is running/)).toBeTruthy();
  });
});
