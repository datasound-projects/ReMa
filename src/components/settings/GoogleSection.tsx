import { useRef, useState } from 'react';

import { useAction } from '../../hooks/useAction';
import { dataOr } from '../../hooks/useAsyncData';
import { useGoogleStatus } from '../../hooks/useGoogle';
import {
  cancelGoogleConnect,
  connectGoogle,
  disconnectGoogle,
  saveGoogleClient,
  setGoogleServiceEnabled,
  type GoogleService,
  type GoogleStatus,
} from '../../services/googleService';
import { StatusIndicator } from '../ui/StatusIndicator';
import { Switch } from '../ui/Switch';

const SERVICES: { id: GoogleService; name: string; description: string }[] = [
  { id: 'gmail', name: 'Gmail', description: 'Read-only: finds job-application emails' },
  { id: 'calendar', name: 'Calendar', description: 'Adds confirmed interviews, checks conflicts' },
];

/** Google Workspace: one sign-in for Gmail and Calendar. */
export function GoogleSection() {
  const google = useGoogleStatus();
  const status = dataOr(google.state, null);
  const [editingClient, setEditingClient] = useState(false);
  const action = useAction();
  const cancelled = useRef(false);

  const connect = async () => {
    cancelled.current = false;
    const ok = await action.run(connectGoogle);
    // Cancelling is not an error worth showing.
    if (!ok && cancelled.current) action.clearError();
  };

  const cancel = () => {
    cancelled.current = true;
    void cancelGoogleConnect();
  };

  const missingAccess =
    status?.connected === true &&
    SERVICES.filter((s) => status[s.id].enabled && !status[s.id].granted).map((s) => s.name);

  return (
    <section className="section" aria-labelledby="google-heading">
      <div className="section__head">
        <div className="section__heading">
          <h2 id="google-heading" className="section__title">
            Google Workspace
          </h2>
          <p className="section__description">
            One sign-in for Gmail and Calendar, used by job-application tasks.
          </p>
        </div>
      </div>
      {google.state.status === 'error' && (
        <p className="form-error" role="alert">
          {google.state.error.message}
        </p>
      )}
      {status && (
        <div className="panel panel--list">
          <div className="provider">
            <div className="provider__row">
              <span className="provider__name">Google Account</span>
              <AccountStatus status={status} />
              {status.connected && status.email && (
                <span className="provider__detail">{status.email}</span>
              )}
              <span className="provider__spacer" />
              {status.connecting ? (
                <button type="button" className="button button--ghost" onClick={cancel}>
                  Cancel
                </button>
              ) : status.connected ? (
                <button
                  type="button"
                  className="button button--ghost"
                  disabled={action.busy}
                  onClick={() => void action.run(disconnectGoogle)}
                >
                  Disconnect
                </button>
              ) : (
                status.client &&
                !editingClient && (
                  <button
                    type="button"
                    className="button button--secondary"
                    disabled={action.busy}
                    onClick={() => void connect()}
                  >
                    {status.needsReconnect ? 'Reconnect' : 'Connect'}
                  </button>
                )
              )}
            </div>

            {status.connecting && (
              <p className="form__hint" role="status">
                Finish signing in with Google in your browser, then return to ReMa.
              </p>
            )}
            {status.needsReconnect && !status.connecting && (
              <p className="form__hint">Google ended ReMa’s access. Connect again to continue.</p>
            )}
            {missingAccess && missingAccess.length > 0 && !status.connecting && (
              <div className="form__inline">
                <span className="form__hint">
                  {missingAccess.join(' and ')} {missingAccess.length > 1 ? 'need' : 'needs'} your
                  permission.
                </span>
                <button
                  type="button"
                  className="button button--ghost"
                  disabled={action.busy}
                  onClick={() => void connect()}
                >
                  Reconnect
                </button>
              </div>
            )}

            {(!status.client || editingClient) && !status.connected && (
              <ClientForm
                canCancel={!!status.client}
                onDone={() => setEditingClient(false)}
              />
            )}
            {status.client === 'custom' && !status.connected && !editingClient && !status.connecting && (
              <button
                type="button"
                className="link-button google__client-link"
                onClick={() => setEditingClient(true)}
              >
                Use a different OAuth client
              </button>
            )}
            {action.error && (
              <p className="form-error" role="alert">
                {action.error}
              </p>
            )}
          </div>

          {SERVICES.map((service) => (
            <ServiceRow key={service.id} status={status} {...service} />
          ))}
        </div>
      )}
      <p className="form__hint">
        ReMa signs in with Google OAuth. Tokens stay in your system keychain and are never shown to
        a model. Only emails that look job-related are read, and only relevant ones reach the model.
      </p>
    </section>
  );
}

function AccountStatus({ status }: { status: GoogleStatus }) {
  if (status.connecting) return <StatusIndicator tone="pending" label="Waiting for sign-in" live />;
  if (status.connected) return <StatusIndicator tone="ready" label="Connected" />;
  if (status.needsReconnect) return <StatusIndicator tone="error" label="Reconnect needed" />;
  return <StatusIndicator tone="idle" label="Not connected" />;
}

function ServiceRow({
  status,
  id,
  name,
  description,
}: {
  status: GoogleStatus;
  id: GoogleService;
  name: string;
  description: string;
}) {
  const action = useAction();
  const service = status[id];
  // The switch shows on/off; the indicator only speaks up when connected.
  const indicator =
    service.enabled && status.connected ? (
      service.granted ? (
        <StatusIndicator tone="ready" label="Ready" />
      ) : (
        <StatusIndicator tone="error" label="No access" />
      )
    ) : null;
  return (
    <div className="provider">
      <div className="setting-row setting-row--flush">
        <div className="setting-row__text">
          <span className="setting-row__label">{name}</span>
          <span className="setting-row__hint">{description}</span>
        </div>
        {indicator}
        <Switch
          checked={service.enabled}
          disabled={action.busy}
          aria-label={`Use ${name}`}
          onChange={(on) => void action.run(() => setGoogleServiceEnabled(id, on))}
        />
      </div>
      {action.error && <p className="form-error">{action.error}</p>}
    </div>
  );
}

/** The user's own "Desktop app" OAuth client from Google Cloud. */
function ClientForm({ canCancel, onDone }: { canCancel: boolean; onDone: () => void }) {
  const [clientId, setClientId] = useState('');
  const [secret, setSecret] = useState('');
  const action = useAction();

  return (
    <form
      className="provider__form"
      onSubmit={(e) => {
        e.preventDefault();
        void action.run(() => saveGoogleClient(clientId, secret)).then((ok) => ok && onDone());
      }}
    >
      <div className="form__row">
        <label className="field field--grow">
          <span className="field__label">OAuth client ID</span>
          <input
            className="input"
            spellCheck={false}
            autoComplete="off"
            placeholder="…apps.googleusercontent.com"
            value={clientId}
            onChange={(e) => setClientId(e.target.value)}
          />
        </label>
        <label className="field field--grow">
          <span className="field__label">Client secret</span>
          <input
            type="password"
            className="input"
            autoComplete="off"
            value={secret}
            onChange={(e) => setSecret(e.target.value)}
          />
        </label>
      </div>
      <div className="form__inline">
        <button type="submit" className="button button--primary" disabled={action.busy || !clientId.trim()}>
          Save
        </button>
        {canCancel && (
          <button type="button" className="button button--ghost" onClick={onDone}>
            Cancel
          </button>
        )}
      </div>
      <p className="form__hint">
        Create an OAuth client of type “Desktop app” in Google Cloud Console and enable the Gmail and
        Google Calendar APIs. The secret is stored in your system keychain.
      </p>
      {action.error && (
        <p className="form-error" role="alert">
          {action.error}
        </p>
      )}
    </form>
  );
}
