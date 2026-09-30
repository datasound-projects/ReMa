import { useRef, useState } from 'react';
import { testingNote } from '../../lib/connectorNotes';
import { LoadFailed, LoadingRows } from '../ui/LoadingRows';

import { useNavigation } from '../../app/navigation';
import { useAction } from '../../hooks/useAction';
import { useScrollIntoFocus } from '../../hooks/useScrollIntoFocus';
import { dataOr } from '../../hooks/useAsyncData';
import { useConnectors } from '../../hooks/useConnectors';
import { useTasks } from '../../hooks/useTasks';
import { formatDateTime, formatRelative } from '../../lib/format';
import {
  cancelConnectorSignIn,
  connectConnector,
  connectProviderAccount,
  disconnectConnector,
  disconnectProviderAccount,
  setBackgroundSettings,
  setConnectionPreferences,
  type ConnectorStatus,
  type ConnectorsOverview,
  type MailProcessing,
  type ProviderAccount,
  type ProviderId,
} from '../../services/connectorService';
import { openExternalUrl } from '../../services/systemService';
import { runTaskNow, type ScheduledTask } from '../../services/taskService';
import { AlertIcon, CheckIcon, ChevronDownIcon, ChevronRightIcon, PlusIcon } from '../icons';
import { Dialog } from '../ui/Dialog';
import { IconButton } from '../ui/IconButton';
import { StatusIndicator } from '../ui/StatusIndicator';
import { Switch } from '../ui/Switch';
import { ConnectorLogo, ProviderLogo } from './ConnectorIcons';
import { NetworkContactsCard } from './NetworkContactsCard';

const MICROSOFT_APPS_URL = 'https://account.microsoft.com/privacy/app-access';
const GOOGLE_APPS_URL = 'https://myaccount.google.com/connections';
const LINKEDIN_APPS_URL = 'https://www.linkedin.com/psettings/permitted-services';

/** "Job Mail & Interview Sync" when it is set up and turned on. */
function useMailTask(): ScheduledTask | null | undefined {
  const tasks = useTasks();
  if (tasks.state.status !== 'success') return undefined;
  return tasks.state.data.find((t) => t.builtin === 'job_mail_sync' && t.enabled) ?? null;
}

/** Whether "Job Mail & Interview Sync" is set up and turned on. */
function useMailTracking(): boolean | null {
  const task = useMailTask();
  return task === undefined ? null : task !== null;
}

/** A sign-in or connection check failed (Retry), as opposed to a sync. */

const signInFailed = (c: ConnectorStatus) =>
  c.state === 'error' && c.errorCode !== null && c.errorCode !== 'CREDENTIAL_STORE_UNAVAILABLE';

/** Where job-related mail goes to be read (Spec B §64), stated plainly. */
function mailPrivacy(mail: MailProcessing | null): string {
  const tokens = 'Sign-in tokens never reach the interface or a model.';
  if (!mail) return `Only job-related email is read in full, and only by the model you choose. ${tokens}`;
  if (mail.onDevice) {
    return `Job-related email is read by ${mail.model} on this computer: mail leaves it only between ReMa and Google or Microsoft. ${tokens}`;
  }
  return `Job-related email text is sent to ${mail.recipient} (${mail.model}) to be read; other mail is filtered on this computer and never sent. ${tokens}`;
}

/** Whether a connector counts as added (it may still need attention). */
const added = (c: ConnectorStatus) => c.enabled && c.state !== 'disconnected';

/** Settings → Connectors: Gmail, Google Calendar, Outlook Mail, Outlook Calendar. */
export function ConnectorsSection({ focus = false }: { focus?: boolean }) {
  const remote = useConnectors();
  const ref = useRef<HTMLElement>(null);
  useScrollIntoFocus(ref, focus);
  const overview = dataOr(remote.state, null);
  const [openId, setOpenId] = useState<ConnectorStatus['id'] | null>(null);
  const [managing, setManaging] = useState<ProviderId | null>(null);
  const open = overview?.connectors.find((c) => c.id === openId) ?? null;
  const managed = overview?.accounts.find((a) => a.provider === managing) ?? null;

  return (
    <section className="section" aria-labelledby="connectors-heading" ref={ref} id="settings-connectors">
      <div className="section__head">
        <div className="section__heading">
          <h2 id="connectors-heading" className="section__title">
            Connectors
          </h2>
          <p className="section__description">
            Connect your mailbox and calendar so ReMa can track job applications and interviews, and your professional
            network for Network Connect. You sign in with the provider in your browser; ReMa never sees your password,
            and its access stays in your system keychain.
          </p>
        </div>
      </div>
      {remote.state.status === 'error' && (
        <LoadFailed message={remote.state.error.message} onRetry={remote.retry} />
      )}
      {remote.state.status === 'loading' && (
        <div className="panel panel--list">
          <LoadingRows count={2} label="Loading connectors…" />
        </div>
      )}
      {overview && (
        <>
          <div className="connector-grid">
            {overview.accounts
              .filter((a) => a.provider === 'google' || a.provider === 'microsoft')
              .map((account) => (
                <AccountCard key={account.provider} account={account} onManage={() => setManaging(account.provider)} />
              ))}
          </div>
          <h3 className="connectors__group">Professional networks</h3>
          <div className="connector-grid">
            {overview.connectors
              .filter((c) => c.kind === 'network')
              .map((connector) => (
                <ConnectorCard key={connector.id} connector={connector} onOpen={() => setOpenId(connector.id)} />
              ))}
          </div>
          <NetworkContactsCard />
          <MailTrackingNote />
          <ChatDefaults overview={overview} />
          <BackgroundOptions overview={overview} />
          <p className="form__hint connectors__privacy">{mailPrivacy(overview.mailProcessing)}</p>
        </>
      )}
      {open && overview && (
        <ConnectorDetail connector={open} overview={overview} onClose={() => setOpenId(null)} />
      )}
      {managed && overview && (
        <AccountDetail account={managed} overview={overview} onClose={() => setManaging(null)} />
      )}
    </section>
  );
}

/** Connect (or reconnect) a whole account through the browser; a cancel is not shown as an error. */
function useConnectAccount(provider: ProviderId) {
  const action = useAction();
  const cancelled = useRef(false);
  const connect = async () => {
    cancelled.current = false;
    const ok = await action.run(() => connectProviderAccount(provider));
    if (!ok && cancelled.current) action.clearError();
  };
  const cancel = () => {
    cancelled.current = true;
    void cancelConnectorSignIn(provider).catch(() => {});
  };
  return { ...action, connect, cancel };
}

const ACCOUNT_STATUS: Record<ProviderAccount['state'], { tone: 'ready' | 'pending' | 'error' | 'idle'; label: string }> = {
  disconnected: { tone: 'idle', label: 'Not connected' },
  connecting: { tone: 'pending', label: 'Waiting for sign-in' },
  connected: { tone: 'ready', label: 'Connected' },
  refreshing: { tone: 'pending', label: 'Renewing access' },
  reauth_required: { tone: 'error', label: 'Reconnect required' },
  permission_denied: { tone: 'error', label: 'Permission missing' },
  admin_approval_required: { tone: 'error', label: 'Approval required' },
  provider_error: { tone: 'error', label: 'Connection failed' },
  offline: { tone: 'error', label: 'Offline' },
  unavailable: { tone: 'idle', label: 'Unavailable' },
};

/** What an account gives ReMa, said before connecting. */
function accountPitch(provider: ProviderId): string {
  return provider === 'google'
    ? 'Gmail and Google Calendar: track job mail and manage confirmed interviews.'
    : 'Outlook Mail and Outlook Calendar: track job mail and manage confirmed interviews.';
}

/** An account's connection needs the user (a new sign-in or the provider's fix). */
const needsAttention = (a: ProviderAccount) =>
  a.state === 'reauth_required' ||
  a.state === 'permission_denied' ||
  a.state === 'admin_approval_required' ||
  a.state === 'provider_error' ||
  a.state === 'offline';

/** One provider account: Google or Microsoft (Settings → Connectors). */
function AccountCard({ account: a, onManage }: { account: ProviderAccount; onManage: () => void }) {
  const connect = useConnectAccount(a.provider);
  const action = useAction();
  const [confirming, setConfirming] = useState(false);
  const [showDetail, setShowDetail] = useState(false);
  const status = ACCOUNT_STATUS[a.state];
  const hasAccount = a.email !== null || a.connectionId !== null;
  const attention = needsAttention(a);
  const live = a.state === 'connected' || a.state === 'refreshing';

  return (
    <div className={`connector-card account-card${attention ? ' connector-card--attention' : ''}`}>
      <div className="connector-card__row">
        <span className="connector-card__logo">
          <ProviderLogo id={a.provider} size={32} />
        </span>
        <span className="connector-card__text">
          <span className="connector-card__name">{a.name}</span>
        </span>
        <StatusIndicator tone={status.tone} label={status.label} live={a.state === 'connecting' || a.state === 'refreshing'} />
      </div>
      {hasAccount && a.email && <p className="connector-card__meta account-card__email">{a.email}</p>}
      {!hasAccount && a.state !== 'connecting' && a.state !== 'unavailable' && (
        <p className="connector-card__description">{accountPitch(a.provider)}</p>
      )}
      {a.state === 'connecting' && (
        <p className="connector-card__note">{a.message ?? `Finish signing in with ${a.name} in your browser.`}</p>
      )}
      {hasAccount && (
        <ul className="account-card__capabilities" aria-label={`${a.name} capabilities`}>
          {a.capabilities.map((c) => (
            <li key={c.connector} className={c.granted ? 'is-granted' : 'is-missing'}>
              {c.granted ? (
                <CheckIcon className="account-card__capability-icon" aria-hidden="true" />
              ) : (
                <span className="account-card__capability-off" aria-hidden="true" />
              )}
              <span>{c.name}</span>
              <span className="sr-only">{c.granted ? ' (on)' : ' (not added)'}</span>
            </li>
          ))}
        </ul>
      )}
      {a.signInEndsAt !== null && !attention && (
        <p className="connector-card__note connector-card__note--muted">{testingNote(a.signInEndsAt)}</p>
      )}
      {(attention || a.state === 'unavailable' || (a.errorCode !== null && a.message)) && a.message && (
        <p className={a.state === 'unavailable' ? 'connector-card__note connector-card__note--muted' : 'connector-card__problem'}>
          {a.state !== 'unavailable' && <AlertIcon className="connector-card__problem-icon" aria-hidden="true" />}
          <span>
            {a.state === 'provider_error' && !hasAccount && <strong>Connection failed. </strong>}
            {a.message}
          </span>
        </p>
      )}
      {(attention || a.errorCode !== null) && a.detail && (
        <>
          <button
            type="button"
            className="link-button connector-card__details-toggle"
            aria-expanded={showDetail}
            onClick={() => setShowDetail(!showDetail)}
          >
            {showDetail ? 'Hide details' : 'Show details'}
          </button>
          {showDetail && <pre className="connector-card__details">{a.detail}</pre>}
        </>
      )}
      <div className="account-card__actions">
        {a.state === 'connecting' ? (
          <button type="button" className="button button--ghost button--small" onClick={connect.cancel}>
            Cancel
          </button>
        ) : a.state === 'unavailable' ? null : !hasAccount ? (
          <button
            type="button"
            className="button button--primary button--small"
            disabled={connect.busy}
            onClick={() => void connect.connect()}
          >
            {a.state === 'provider_error' || a.state === 'admin_approval_required' || a.state === 'offline' || a.state === 'permission_denied'
              ? 'Retry'
              : 'Connect'}
          </button>
        ) : (
          <>
            {attention && (
              <button
                type="button"
                className="button button--primary button--small"
                disabled={connect.busy}
                onClick={() => void connect.connect()}
              >
                Reconnect
              </button>
            )}
            <button type="button" className="button button--secondary button--small" disabled={!live && !attention} onClick={onManage}>
              Manage
            </button>
            <button
              type="button"
              className="button button--ghost button--small"
              disabled={action.busy}
              onClick={() => setConfirming(true)}
            >
              Disconnect
            </button>
          </>
        )}
      </div>
      {(connect.error ?? action.error) && a.errorCode === null && a.state !== 'connecting' && (
        <p className="form-error" role="alert">
          {connect.error ?? action.error}
        </p>
      )}
      {confirming && (
        <DisconnectAccountDialog
          account={a}
          busy={action.busy}
          error={action.error}
          onCancel={() => setConfirming(false)}
          onConfirm={() =>
            void action.run(() => disconnectProviderAccount(a.provider)).then((ok) => {
              if (ok) setConfirming(false);
            })
          }
        />
      )}
    </div>
  );
}

function DisconnectAccountDialog({
  account: a,
  busy,
  error,
  onCancel,
  onConfirm,
}: {
  account: ProviderAccount;
  busy: boolean;
  error: string | null;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  return (
    <Dialog
      title={`Disconnect ${a.name}?`}
      onClose={onCancel}
      actions={
        <>
          {error && <p className="form-error">{error}</p>}
          <button type="button" className="button button--secondary" onClick={onCancel}>
            Cancel
          </button>
          <button type="button" className="button button--danger" disabled={busy} onClick={onConfirm}>
            Disconnect
          </button>
        </>
      }
    >
      <p className="dialog__text">
        ReMa stops using {a.capabilities.map((c) => c.name).join(' and ')}. Your tracked applications and their history
        stay in ReMa.
      </p>
      <p className="dialog__text">
        {a.provider === 'microsoft'
          ? 'ReMa also deletes its Microsoft sign-in from this computer. To remove ReMa from your Microsoft account entirely, open your account’s app permissions.'
          : 'ReMa also revokes its access at Google and deletes the sign-in from this computer.'}
      </p>
    </Dialog>
  );
}

/** Manage: the account's capabilities (add or remove each), permissions, sync and sign-in. */
function AccountDetail({
  account: a,
  overview,
  onClose,
}: {
  account: ProviderAccount;
  overview: ConnectorsOverview;
  onClose: () => void;
}) {
  const connect = useConnectAccount(a.provider);
  const action = useAction();
  const sync = useAction();
  const mailTask = useMailTask();
  const [removing, setRemoving] = useState<ConnectorStatus | null>(null);
  const [showDetail, setShowDetail] = useState(false);
  const [synced, setSynced] = useState(false);
  const cards = overview.connectors.filter((c) => c.provider === a.provider);
  const microsoft = a.provider === 'microsoft';
  const status = ACCOUNT_STATUS[a.state];
  const permissions = cards.flatMap((c) => c.permissions.map((p) => ({ ...p, connector: c.name })));

  if (removing) {
    return (
      <Dialog
        title={`Remove ${removing.name}?`}
        onClose={() => setRemoving(null)}
        actions={
          <>
            {action.error && <p className="form-error">{action.error}</p>}
            <button type="button" className="button button--secondary" onClick={() => setRemoving(null)}>
              Cancel
            </button>
            <button
              type="button"
              className="button button--danger"
              disabled={action.busy}
              onClick={() =>
                void action.run(() => disconnectConnector(removing.id)).then((ok) => {
                  if (ok) setRemoving(null);
                })
              }
            >
              Remove
            </button>
          </>
        }
      >
        <p className="dialog__text">
          ReMa stops syncing {removing.name}. Your tracked applications and their history stay in ReMa.
        </p>
        <p className="dialog__text">
          {cards.filter((c) => c.id !== removing.id && added(c)).length > 0
            ? `Your ${a.name} account stays connected for the rest.`
            : microsoft
              ? 'It is the last capability of this account: ReMa also deletes its Microsoft sign-in from this computer.'
              : 'It is the last capability of this account: ReMa also revokes its access at Google and deletes the sign-in from this computer.'}
        </p>
      </Dialog>
    );
  }

  return (
    <Dialog
      title={`${a.name} account`}
      size="wide"
      onClose={onClose}
      actions={
        <button type="button" className="button button--secondary" onClick={onClose}>
          Close
        </button>
      }
    >
      <div className="connector-detail">
        <div className="connector-detail__head">
          <ProviderLogo id={a.provider} size={40} />
          <div className="connector-detail__heading">
            <span className="connector-detail__account">{a.displayName ?? a.email ?? `${a.name} account`}</span>
            {a.email && a.displayName && <span className="connector-detail__email">{a.email}</span>}
          </div>
          <StatusIndicator tone={status.tone} label={status.label} />
        </div>
        {a.message && <p className="connector-detail__message">{a.message}</p>}

        <dl className="connector-detail__facts">
          <dt>Capabilities</dt>
          <dd>
            <ul className="connector-detail__capabilities">
              {cards.map((c) => (
                <li key={c.id}>
                  <span>{c.name}</span>
                  <span className="connector-detail__capability-state">{capabilityState(c)}</span>
                  {added(c) ? (
                    <button
                      type="button"
                      className="button button--ghost button--small"
                      disabled={action.busy}
                      onClick={() => setRemoving(c)}
                    >
                      Remove {c.name}
                    </button>
                  ) : (
                    <button
                      type="button"
                      className="button button--secondary button--small"
                      disabled={connect.busy || c.state === 'unavailable'}
                      onClick={() => void connect.run(() => connectConnector(c.id))}
                    >
                      Add {c.name}
                    </button>
                  )}
                </li>
              ))}
            </ul>
          </dd>
          <dt>Permissions</dt>
          <dd>
            <ul className="connector-detail__permissions">
              {permissions.map((p) => (
                <li key={`${p.connector}-${p.capability}`} className={p.granted ? 'is-granted' : 'is-missing'}>
                  {p.granted ? (
                    <CheckIcon className="connector-detail__perm-icon" aria-hidden="true" />
                  ) : (
                    <AlertIcon className="connector-detail__perm-icon" aria-hidden="true" />
                  )}
                  {p.label}
                  <span className="sr-only">{p.granted ? ' (granted)' : ' (not granted)'}</span>
                </li>
              ))}
            </ul>
          </dd>
          <dt>Health</dt>
          <dd>
            {a.connectedAt !== null && <span>Signed in {formatDateTime(a.connectedAt)}. </span>}
            {a.lastRefreshedAt !== null
              ? `Access renewed ${formatRelative(a.lastRefreshedAt)}.`
              : 'Access is renewed only when a request needs it.'}
          </dd>
          <dt>Last sync</dt>
          <dd>
            {cards.some((c) => c.lastSyncAt !== null)
              ? cards
                  .filter((c) => c.lastSyncAt !== null)
                  .map((c) => `${c.name}: ${formatDateTime(c.lastSyncAt as number)}`)
                  .join(' · ')
              : 'Not yet'}
          </dd>
        </dl>

        <MailTrackingLine />

        <div className="connector-detail__actions">
          {mailTask && (
            <button
              type="button"
              className="button button--secondary button--small"
              disabled={sync.busy || a.state === 'connecting'}
              onClick={() =>
                void sync.run(() => runTaskNow(mailTask.id)).then((ok) => {
                  if (ok) setSynced(true);
                })
              }
            >
              {sync.busy ? 'Starting…' : 'Sync now'}
            </button>
          )}
          {a.state === 'connecting' ? (
            <button type="button" className="button button--ghost button--small" onClick={connect.cancel}>
              Cancel sign-in
            </button>
          ) : (
            <button
              type="button"
              className="button button--secondary button--small"
              disabled={connect.busy}
              onClick={() => void connect.connect()}
            >
              Reconnect
            </button>
          )}
        </div>
        {synced && (
          <p className="form__hint" role="status">
            Job Mail &amp; Interview Sync is running; see Scheduled Tasks for its result.
          </p>
        )}
        {(action.error ?? sync.error ?? (a.errorCode === null ? connect.error : null)) && (
          <p className="form-error" role="alert">
            {action.error ?? sync.error ?? connect.error}
          </p>
        )}

        {a.detail && (
          <div>
            <button
              type="button"
              className="button button--ghost button--small"
              aria-expanded={showDetail}
              onClick={() => setShowDetail(!showDetail)}
            >
              {showDetail ? <ChevronDownIcon className="button__icon" /> : <ChevronRightIcon className="button__icon" />}
              Technical details
            </button>
            {showDetail && <pre className="connector-card__details">{a.detail}</pre>}
          </div>
        )}
        <p className="form__hint">
          {microsoft ? 'Manage apps with access to your Microsoft account at ' : 'Review apps with access to your Google Account at '}
          <button
            type="button"
            className="link-button"
            onClick={() => void openExternalUrl(microsoft ? MICROSOFT_APPS_URL : GOOGLE_APPS_URL).catch(() => {})}
          >
            {microsoft ? 'account.microsoft.com' : 'myaccount.google.com'}
          </button>
          .
        </p>
      </div>
    </Dialog>
  );
}

/** How a newly connected account meets chats (privacy-conscious default: off). */
function ChatDefaults({ overview }: { overview: ConnectorsOverview }) {
  const action = useAction();
  const { newAccountsInChats } = overview.preferences;
  return (
    <div className="panel panel--list connectors__prefs">
      <div className="setting-row">
        <div className="setting-row__text">
          <span className="setting-row__label">Add new accounts to chats that chose their own connectors</span>
          <span className="setting-row__hint">
            Chats using “all connected” see a new account right away. Off: a chat that picked its own connectors keeps
            that choice until you change it there. Turning a connector off in a chat never disconnects the account.
          </span>
        </div>
        <Switch
          checked={newAccountsInChats}
          disabled={action.busy}
          aria-label="Add new accounts to chats that chose their own connectors"
          onChange={(on) => void action.run(() => setConnectionPreferences({ newAccountsInChats: on }))}
        />
      </div>
      {action.error && <p className="form-error connectors__error">{action.error}</p>}
    </div>
  );
}

/** Connect (or reconnect) through the browser; a cancel is not shown as an error. */
function useConnect(connector: ConnectorStatus) {
  const action = useAction();
  const cancelled = useRef(false);
  const connect = async () => {
    cancelled.current = false;
    const ok = await action.run(() => connectConnector(connector.id));
    if (!ok && cancelled.current) action.clearError();
  };
  const cancel = () => {
    cancelled.current = true;
    void cancelConnectorSignIn(connector.provider).catch(() => {});
  };
  return { ...action, connect, cancel };
}

function ConnectorCard({ connector: c, onOpen }: { connector: ConnectorStatus; onOpen: () => void }) {
  const connect = useConnect(c);
  const [showDetail, setShowDetail] = useState(false);
  const isAdded = added(c);
  const needsReconnect = c.state === 'reauth_required' || c.state === 'permission_missing';

  let status;
  switch (c.state) {
    case 'disconnected':
      status = (
        <IconButton
          label={`Add ${c.name}`}
          className="connector-card__add"
          disabled={connect.busy}
          onClick={() => void connect.connect()}
        >
          <PlusIcon />
        </IconButton>
      );
      break;
    case 'connecting':
      status = (
        <span className="connector-card__status" role="status">
          <span className="spinner" aria-hidden="true" />
          <button type="button" className="link-button" onClick={connect.cancel}>
            Cancel
          </button>
        </span>
      );
      break;
    case 'connected':
      status = (
        <span className="connector-card__status connector-card__status--ok" title="Connected">
          <CheckIcon className="connector-card__check" aria-hidden="true" />
          <span className="sr-only">Connected</span>
        </span>
      );
      break;
    case 'syncing':
      status = (
        <span className="connector-card__status" role="status">
          <span className="spinner" aria-hidden="true" />
          <span className="connector-card__status-text">Syncing…</span>
        </span>
      );
      break;
    case 'reauth_required':
    case 'permission_missing':
      status = (
        <button
          type="button"
          className="button button--secondary button--small"
          disabled={connect.busy}
          onClick={() => void connect.connect()}
        >
          Reconnect
        </button>
      );
      break;
    case 'error':
      if (signInFailed(c)) {
        // Connecting or its check failed: Retry opens the sign-in again.
        status = (
          <button
            type="button"
            className="button button--secondary button--small"
            disabled={connect.busy}
            onClick={() => void connect.connect()}
          >
            Retry
          </button>
        );
      } else if (c.errorCode === 'CREDENTIAL_STORE_UNAVAILABLE') {
        status = <StatusIndicator tone="error" label="Keychain unavailable" />;
      } else {
        // The last run of a task that used it failed; the next run retries.
        status = <StatusIndicator tone="error" label="Last sync failed" />;
      }
      break;
    case 'unavailable':
      status = <StatusIndicator tone="idle" label="Unavailable" />;
      break;
  }

  const attention = needsReconnect || c.state === 'error';
  return (
    <div className={`connector-card${attention ? ' connector-card--attention' : ''}`}>
      <div className="connector-card__row">
        <button
          type="button"
          className="connector-card__open"
          disabled={!isAdded}
          aria-label={isAdded ? `${c.name} details` : undefined}
          onClick={onOpen}
        >
          <span className="connector-card__logo">
            <ConnectorLogo id={c.id} size={32} />
          </span>
          <span className="connector-card__text">
            <span className="connector-card__name">{c.name}</span>
            <span className="connector-card__by">by {c.publisher}</span>
          </span>
        </button>
        {status}
      </div>
      <p className="connector-card__description">{c.description}</p>
      {c.state === 'connecting' && (
        <p className="connector-card__note">{c.message ?? `Finish signing in with ${c.publisher} in your browser.`}</p>
      )}
      {isAdded && c.state !== 'connecting' && (
        <p className="connector-card__meta">
          {c.accountEmail && <span>{c.accountEmail}</span>}
          {c.lastSyncAt !== null && <span>Synced {formatRelative(c.lastSyncAt)}</span>}
        </p>
      )}
      {c.signInEndsAt !== null && !attention && (
        <p className="connector-card__note connector-card__note--muted">{testingNote(c.signInEndsAt)}</p>
      )}
      {/* A network with no API for ReMa (XING) is a fact, not a fault. */}
      {c.kind === 'network' && c.state === 'unavailable' && c.errorCode === null && c.message && (
        <p className="connector-card__note connector-card__note--muted">{c.message}</p>
      )}
      {(attention || (c.state === 'unavailable' && c.kind !== 'network') || (c.errorCode !== null && c.message)) &&
        c.message && (
        <p className="connector-card__problem">
          <AlertIcon className="connector-card__problem-icon" aria-hidden="true" />
          <span>
            {signInFailed(c) && <strong>Connection failed. </strong>}
            {c.message}
          </span>
        </p>
      )}
      {attention && c.detail && (
        <>
          <button
            type="button"
            className="link-button connector-card__details-toggle"
            aria-expanded={showDetail}
            onClick={() => setShowDetail(!showDetail)}
          >
            {showDetail ? 'Hide details' : 'Show details'}
          </button>
          {showDetail && <pre className="connector-card__details">{c.detail}</pre>}
        </>
      )}
      {/* A failed sign-in is reported on the card itself. */}
      {connect.error && c.errorCode === null && c.state !== 'connecting' && (
        <p className="form-error" role="alert">
          {connect.error}
        </p>
      )}
    </div>
  );
}

function ConnectorDetail({
  connector: c,
  overview,
  onClose,
}: {
  connector: ConnectorStatus;
  overview: ConnectorsOverview;
  onClose: () => void;
}) {
  const connect = useConnect(c);
  const action = useAction();
  const sync = useAction();
  const mailTask = useMailTask();
  const [confirming, setConfirming] = useState(false);
  const [showDetail, setShowDetail] = useState(false);
  const [synced, setSynced] = useState(false);
  const siblings = overview.connectors.filter((o) => o.provider === c.provider && o.id !== c.id && added(o));
  const account = overview.connectors.filter((o) => o.provider === c.provider);
  const lastOfAccount = siblings.length === 0;
  const microsoft = c.provider === 'microsoft';
  const linkedin = c.provider === 'linkedin';

  if (confirming) {
    return (
      <Dialog
        title={`Disconnect ${c.name}?`}
        onClose={() => setConfirming(false)}
        actions={
          <>
            {action.error && <p className="form-error">{action.error}</p>}
            <button type="button" className="button button--secondary" onClick={() => setConfirming(false)}>
              Cancel
            </button>
            <button
              type="button"
              className="button button--danger"
              disabled={action.busy}
              onClick={() =>
                void action.run(() => disconnectConnector(c.id)).then((ok) => {
                  if (ok) onClose();
                })
              }
            >
              Disconnect
            </button>
          </>
        }
      >
        <p className="dialog__text">
          {c.kind === 'network'
            ? `Network Connect stops using ${c.name}. Company, job and public people research keep working.`
            : `ReMa stops syncing ${c.name}. Your tracked applications and their history stay in ReMa.`}
        </p>
        {lastOfAccount ? (
          <p className="dialog__text">
            {microsoft
              ? 'ReMa also deletes its Microsoft sign-in from this computer. To remove ReMa from your Microsoft account entirely, open your account’s app permissions.'
              : linkedin
                ? 'ReMa also deletes its LinkedIn sign-in from this computer and forgets anything LinkedIn returned this session. To remove ReMa from your LinkedIn account entirely, open LinkedIn’s permitted services.'
                : 'ReMa also revokes its access at Google and deletes the sign-in from this computer.'}
          </p>
        ) : (
          <p className="dialog__text">
            {siblings.map((s) => s.name).join(' and ')} stays connected with the same {c.publisher} account.
          </p>
        )}
      </Dialog>
    );
  }

  return (
    <Dialog
      title={c.name}
      size="wide"
      onClose={onClose}
      actions={
        <button type="button" className="button button--secondary" onClick={onClose}>
          Close
        </button>
      }
    >
      <div className="connector-detail">
        <div className="connector-detail__head">
          <ConnectorLogo id={c.id} size={40} />
          <div className="connector-detail__heading">
            <span className="connector-detail__account">{c.accountName ?? c.accountEmail ?? `${c.publisher} account`}</span>
            {c.accountEmail && c.accountName && <span className="connector-detail__email">{c.accountEmail}</span>}
          </div>
          <DetailStatus connector={c} />
        </div>
        {c.message && <p className="connector-detail__message">{c.message}</p>}

        <dl className="connector-detail__facts">
          {c.kind !== 'network' && (
            <>
              <dt>Capabilities</dt>
              <dd>
                <ul className="connector-detail__capabilities">
                  {account.map((o) => (
                    <li key={o.id}>
                      <span>{o.name}</span>
                      <span className="connector-detail__capability-state">{capabilityState(o)}</span>
                    </li>
                  ))}
                </ul>
              </dd>
            </>
          )}
          <dt>Permissions</dt>
          <dd>
            <ul className="connector-detail__permissions">
              {c.permissions.map((p) => (
                <li key={p.capability} className={p.granted ? 'is-granted' : 'is-missing'}>
                  {p.granted ? (
                    <CheckIcon className="connector-detail__perm-icon" aria-hidden="true" />
                  ) : (
                    <AlertIcon className="connector-detail__perm-icon" aria-hidden="true" />
                  )}
                  {p.label}
                  <span className="sr-only">{p.granted ? ' (granted)' : ' (not granted)'}</span>
                </li>
              ))}
            </ul>
          </dd>
          {c.kind !== 'network' && (
            <>
              <dt>Last sync</dt>
              <dd>{c.lastSyncAt !== null ? formatDateTime(c.lastSyncAt) : 'Not yet'}</dd>
            </>
          )}
        </dl>
        {c.kind === 'network' && (
          <p className="form__hint">
            Signing in gives ReMa your identity only. Anything more (such as your first-degree connection list) exists
            only if LinkedIn approved it for ReMa; Network Connect shows exactly what is available.
          </p>
        )}

        {c.kind === 'mail' && <MailTrackingLine />}

        <div className="connector-detail__actions">
          {c.kind !== 'network' && mailTask && (
            // Runs the built-in Job Mail & Interview Sync now: the only way
            // ReMa reads mail, turned on by the user.
            <button
              type="button"
              className="button button--secondary button--small"
              disabled={sync.busy || c.state === 'connecting'}
              onClick={() =>
                void sync.run(() => runTaskNow(mailTask.id)).then((ok) => {
                  if (ok) setSynced(true);
                })
              }
            >
              {sync.busy ? 'Starting…' : 'Sync now'}
            </button>
          )}
          {c.state === 'connecting' ? (
            <button type="button" className="button button--ghost button--small" onClick={connect.cancel}>
              Cancel sign-in
            </button>
          ) : (
            <button
              type="button"
              className="button button--secondary button--small"
              disabled={connect.busy}
              onClick={() => void connect.connect()}
            >
              Reconnect
            </button>
          )}
          <span className="provider__spacer" />
          <button type="button" className="button button--ghost button--small" onClick={() => setConfirming(true)}>
            Disconnect
          </button>
        </div>
        {synced && (
          <p className="form__hint" role="status">
            Job Mail &amp; Interview Sync is running; see Scheduled Tasks for its result.
          </p>
        )}
        {(action.error ?? sync.error ?? (c.errorCode === null ? connect.error : null)) && (
          <p className="form-error" role="alert">
            {action.error ?? sync.error ?? connect.error}
          </p>
        )}

        {c.detail && (
          <div>
            <button
              type="button"
              className="button button--ghost button--small"
              aria-expanded={showDetail}
              onClick={() => setShowDetail(!showDetail)}
            >
              {showDetail ? <ChevronDownIcon className="button__icon" /> : <ChevronRightIcon className="button__icon" />}
              Technical details
            </button>
            {showDetail && <pre className="connector-card__details">{c.detail}</pre>}
          </div>
        )}
        <p className="form__hint">
          {microsoft
            ? 'Manage apps with access to your Microsoft account at '
            : linkedin
              ? 'Manage apps with access to your LinkedIn account at '
              : 'Review apps with access to your Google Account at '}
          <button
            type="button"
            className="link-button"
            onClick={() =>
              void openExternalUrl(microsoft ? MICROSOFT_APPS_URL : linkedin ? LINKEDIN_APPS_URL : GOOGLE_APPS_URL).catch(
                () => {},
              )
            }
          >
            {microsoft ? 'account.microsoft.com' : linkedin ? 'linkedin.com' : 'myaccount.google.com'}
          </button>
          .
        </p>
      </div>
    </Dialog>
  );
}

function DetailStatus({ connector: c }: { connector: ConnectorStatus }) {
  switch (c.state) {
    case 'connected':
      return <StatusIndicator tone="ready" label="Connected" />;
    case 'syncing':
      return <StatusIndicator tone="pending" label="Syncing" live />;
    case 'connecting':
      return <StatusIndicator tone="pending" label="Waiting for sign-in" live />;
    case 'reauth_required':
      return <StatusIndicator tone="error" label="Reconnect required" />;
    case 'permission_missing':
      return <StatusIndicator tone="error" label="Permission missing" />;
    case 'error':
      return <StatusIndicator tone="error" label={signInFailed(c) ? 'Connection failed' : 'Sync failed'} />;
    default:
      return <StatusIndicator tone="idle" label="Not connected" />;
  }
}

/** One connector of the account, in the details' capability list. */
function capabilityState(c: ConnectorStatus): string {
  switch (c.state) {
    case 'connected':
    case 'syncing':
      return 'Connected';
    case 'connecting':
      return 'Signing in…';
    case 'reauth_required':
      return 'Reconnect required';
    case 'permission_missing':
      return 'Permission missing';
    case 'error':
      return signInFailed(c) ? 'Connection failed' : 'Needs attention';
    case 'unavailable':
      return 'Unavailable';
    default:
      return 'Not added';
  }
}

/** Mail is read only by the built-in task, never because it is connected. */
function MailTrackingNote() {
  const { navigate } = useNavigation();
  const tracking = useMailTracking();
  return (
    <div className="panel panel--list connectors__prefs">
      <div className="setting-row">
        <div className="setting-row__text">
          <span className="setting-row__label">Job mail tracking</span>
          <span className="setting-row__hint">
            Connecting a mailbox never starts reading it. ReMa tracks job mail and adds confirmed interviews to your
            calendar only while the scheduled task Job Mail &amp; Interview Sync is turned on.
          </span>
        </div>
        <span className="connectors__tracking">
          {tracking !== null && <StatusIndicator tone={tracking ? 'ready' : 'idle'} label={tracking ? 'On' : 'Off'} />}
          <button type="button" className="button button--secondary button--small" onClick={() => navigate({ page: 'tasks' })}>
            Scheduled Tasks
          </button>
        </span>
      </div>
    </div>
  );
}

function MailTrackingLine() {
  const tracking = useMailTracking();
  if (tracking === null) return null;
  return (
    <p className="form__hint">
      {tracking
        ? 'Read by Job Mail & Interview Sync on its schedule (Scheduled Tasks).'
        : 'Not read automatically: turn on Job Mail & Interview Sync in Scheduled Tasks to track job mail.'}
    </p>
  );
}

function BackgroundOptions({ overview }: { overview: ConnectorsOverview }) {
  const action = useAction();
  const { runInBackground, startAtLogin, trayAvailable } = overview.background;
  return (
    <div className="panel panel--list connectors__background">
      <div className="setting-row">
        <div className="setting-row__text">
          <span className="setting-row__label">Run ReMa in background</span>
          <span className="setting-row__hint">
            {trayAvailable
              ? 'Closing the window keeps ReMa in the system tray, so scheduled tasks keep running. Quit from the tray icon.'
              : 'Not available: this desktop has no system tray.'}
          </span>
        </div>
        <Switch
          checked={runInBackground}
          disabled={action.busy || (!trayAvailable && !runInBackground)}
          aria-label="Run ReMa in background"
          onChange={(on) => void action.run(() => setBackgroundSettings(on, startAtLogin))}
        />
      </div>
      <div className="setting-row">
        <div className="setting-row__text">
          <span className="setting-row__label">Start ReMa at login</span>
          <span className="setting-row__hint">
            ReMa opens {runInBackground ? 'in the tray' : ''} when you log in. When ReMa is not running, no scheduled
            task runs; no background service is installed.
          </span>
        </div>
        <Switch
          checked={startAtLogin}
          disabled={action.busy}
          aria-label="Start ReMa at login"
          onChange={(on) => void action.run(() => setBackgroundSettings(runInBackground, on))}
        />
      </div>
      {action.error && <p className="form-error connectors__error">{action.error}</p>}
    </div>
  );
}
