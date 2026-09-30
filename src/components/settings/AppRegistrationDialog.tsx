import { useState } from 'react';

import { useAction } from '../../hooks/useAction';
import {
  removeAppRegistration,
  setAppRegistration,
  type AppRegistration,
  type ProviderId,
} from '../../services/connectorService';
import { openExternalUrl } from '../../services/systemService';
import { Dialog } from '../ui/Dialog';

/** The registration guide in the repository (docs/connectors/registration.md). */
export const REGISTRATION_GUIDE_URL =
  'https://github.com/datasound-projects/ReMa/blob/HEAD/docs/connectors/registration.md';

const NAMES: Record<Exclude<ProviderId, 'xing'>, string> = {
  google: 'Google',
  microsoft: 'Microsoft',
  linkedin: 'LinkedIn',
};

/** How to get the values, in the provider's own console. */
const STEPS: Record<Exclude<ProviderId, 'xing'>, string[]> = {
  google: [
    'In Google Cloud Console, enable the Gmail API and the Google Calendar API and fill in the OAuth consent screen (Google Auth Platform).',
    'Under Clients, create an OAuth client of type Desktop app. No redirect URI is needed.',
    'Paste its client ID below, and the client secret if Google issued one. Google accounts then connect with Connect.',
  ],
  microsoft: [
    'In the Microsoft Entra admin center, register an app for accounts in any organizational directory and personal Microsoft accounts.',
    'Under Authentication, add the platform Mobile and desktop applications with the redirect URI http://localhost. Do not create a client secret.',
    'Under API permissions, add the delegated Microsoft Graph permissions openid, profile, email, offline_access, User.Read, Mail.Read and Calendars.ReadWrite. Paste the Application (client) ID below.',
  ],
  linkedin: [
    'In the LinkedIn Developer Portal, create an app and add the product Sign In with LinkedIn using OpenID Connect.',
    'Ask LinkedIn to enable the native PKCE flow for the app (ReMa never uses a client secret).',
    'Paste its client ID below.',
  ],
};

const CLIENT_ID: Record<Exclude<ProviderId, 'xing'>, { label: string; placeholder: string; hint: string }> = {
  google: {
    label: 'Client ID',
    placeholder: '123456789012-abcdefghijklmnop.apps.googleusercontent.com',
    hint: 'Ends with .apps.googleusercontent.com.',
  },
  microsoft: {
    label: 'Application (client) ID',
    placeholder: '12345678-1234-1234-1234-123456789abc',
    hint: 'A GUID from the app registration’s overview page.',
  },
  linkedin: {
    label: 'Client ID',
    placeholder: '86abcdefghij12',
    hint: 'From the app’s Auth tab.',
  },
};

/**
 * Settings → Connectors → Set up: enter ReMa's app registration for a
 * provider this copy of ReMa was built without. Client IDs stay on this
 * computer; a Google client secret goes to the system keychain.
 */
export function AppRegistrationDialog({
  registration: r,
  onClose,
}: {
  registration: AppRegistration;
  onClose: () => void;
}) {
  const provider = r.provider as Exclude<ProviderId, 'xing'>;
  const name = NAMES[provider];
  const action = useAction();
  const [clientId, setClientId] = useState(r.clientId ?? '');
  const [secret, setSecret] = useState('');
  const [removeSecret, setRemoveSecret] = useState(false);
  const [status, setStatus] = useState(r.publishingStatus);
  const [scopes, setScopes] = useState(r.approvedScopes);
  const [confirmRemove, setConfirmRemove] = useState(false);
  const field = CLIENT_ID[provider];
  const canSave = clientId.trim() !== '' && !action.busy;

  const save = async () => {
    const ok = await action.run(() =>
      setAppRegistration(provider, {
        clientId: clientId.trim(),
        // Empty keeps a stored secret; "remove" clears it explicitly.
        clientSecret: provider !== 'google' ? null : removeSecret ? '' : secret.trim() === '' ? null : secret.trim(),
        publishingStatus: provider === 'google' ? status : '',
        approvedScopes: provider === 'linkedin' ? scopes.trim() : '',
      }),
    );
    if (ok) onClose();
  };

  const remove = async () => {
    const ok = await action.run(() => removeAppRegistration(provider));
    if (ok) onClose();
  };

  if (confirmRemove) {
    return (
      <Dialog
        title={`Remove the ${name} registration?`}
        onClose={() => setConfirmRemove(false)}
        actions={
          <>
            {action.error && <p className="form-error">{action.error}</p>}
            <button type="button" className="button button--secondary" onClick={() => setConfirmRemove(false)}>
              Cancel
            </button>
            <button type="button" className="button button--danger" disabled={action.busy} onClick={() => void remove()}>
              Remove
            </button>
          </>
        }
      >
        <p className="dialog__text">
          {name} sign-in becomes unavailable in this copy of ReMa until a registration is entered again. Nothing
          changes at {name}.
        </p>
      </Dialog>
    );
  }

  return (
    <Dialog
      title={r.source === 'settings' ? `${name} app registration` : `Set up ${name} sign-in`}
      onClose={onClose}
      actions={
        <>
          {action.error && (
            <p className="form-error" role="alert">
              {action.error}
            </p>
          )}
          {r.source === 'settings' && (
            <button type="button" className="button button--ghost" disabled={action.busy} onClick={() => setConfirmRemove(true)}>
              Remove
            </button>
          )}
          <button type="button" className="button button--secondary" onClick={onClose}>
            Cancel
          </button>
          <button type="button" className="button button--primary" disabled={!canSave} onClick={() => void save()}>
            Save
          </button>
        </>
      }
    >
      <p className="dialog__text">
        ReMa signs you in with its own {name} app. This copy of ReMa was built without one, so enter the details of
        a {name} app registration once. ReMa keeps the client ID on this computer
        {provider === 'google' ? ' and the secret in your system keychain' : ''}; your {name} account itself is
        connected afterwards, in your browser, with Connect.
      </p>
      <ol className="registration__steps">
        {STEPS[provider].map((step) => (
          <li key={step}>{step}</li>
        ))}
      </ol>
      <p className="form__hint">
        <button type="button" className="link-button" onClick={() => void openExternalUrl(REGISTRATION_GUIDE_URL)}>
          Open the full registration guide
        </button>
      </p>
      <label className="field">
        <span className="field__label">{field.label}</span>
        <input
          className="input"
          autoFocus
          autoComplete="off"
          spellCheck={false}
          placeholder={field.placeholder}
          value={clientId}
          onChange={(e) => setClientId(e.target.value)}
        />
        <span className="form__hint">{field.hint}</span>
      </label>
      {provider === 'google' && (
        <>
          <label className="field">
            <span className="field__label">Client secret (optional)</span>
            <input
              className="input"
              type="password"
              autoComplete="off"
              placeholder={r.clientSecretSet ? 'Stored in your keychain; leave empty to keep it' : 'Only if the Desktop client has one'}
              value={secret}
              disabled={removeSecret}
              onChange={(e) => setSecret(e.target.value)}
            />
            <span className="form__hint">
              Google marks it optional for desktop apps; some clients still need it. It is stored in your system
              keychain, never shown again.
            </span>
          </label>
          {r.clientSecretSet && (
            <label className="registration__check">
              <input type="checkbox" checked={removeSecret} onChange={(e) => setRemoveSecret(e.target.checked)} />
              <span>Remove the stored secret</span>
            </label>
          )}
          <label className="field">
            <span className="field__label">Publishing status of the Google app</span>
            <select className="input" value={status} onChange={(e) => setStatus(e.target.value)}>
              <option value="">Not sure</option>
              <option value="testing">Testing (Google ends sign-ins after about 7 days)</option>
              <option value="production">In production</option>
            </select>
            <span className="form__hint">
              Google Auth Platform → Audience. In Testing, only listed test users can sign in and ReMa says in advance
              when Google will end a sign-in.
            </span>
          </label>
        </>
      )}
      {provider === 'linkedin' && (
        <label className="field">
          <span className="field__label">Approved scopes (optional)</span>
          <input
            className="input"
            autoComplete="off"
            spellCheck={false}
            placeholder="r_1st_connections"
            value={scopes}
            onChange={(e) => setScopes(e.target.value)}
          />
          <span className="form__hint">
            Only if LinkedIn approved the app for its Connections API; ReMa recognises r_1st_connections.
          </span>
        </label>
      )}
    </Dialog>
  );
}
