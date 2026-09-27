import { useRef, useState } from 'react';

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
  disconnectConnector,
  setBackgroundSettings,
  type ConnectorStatus,
  type ConnectorsOverview,
  type MailProcessing,
} from '../../services/connectorService';
import { openExternalUrl } from '../../services/systemService';
import { runTaskNow, type ScheduledTask } from '../../services/taskService';
import { AlertIcon, CheckIcon, ChevronDownIcon, ChevronRightIcon, PlusIcon } from '../icons';
import { Dialog } from '../ui/Dialog';
import { IconButton } from '../ui/IconButton';
import { StatusIndicator } from '../ui/StatusIndicator';
import { Switch } from '../ui/Switch';
import { ConnectorLogo } from './ConnectorIcons';

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
  const open = overview?.connectors.find((c) => c.id === openId) ?? null;

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
        <p className="form-error" role="alert">
          {remote.state.error.message}
        </p>
      )}
      {overview && (
        <>
          <div className="connector-grid">
            {overview.connectors
              .filter((c) => c.kind !== 'network')
              .map((connector) => (
                <ConnectorCard key={connector.id} connector={connector} onOpen={() => setOpenId(connector.id)} />
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
          <MailTrackingNote />
          <BackgroundOptions overview={overview} />
          <p className="form__hint connectors__privacy">{mailPrivacy(overview.mailProcessing)}</p>
        </>
      )}
      {open && overview && (
        <ConnectorDetail connector={open} overview={overview} onClose={() => setOpenId(null)} />
      )}
    </section>
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
      {(attention || c.state === 'unavailable' || (c.errorCode !== null && c.message)) && c.message && (
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
