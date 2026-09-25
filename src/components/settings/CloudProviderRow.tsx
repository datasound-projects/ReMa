import { useEffect, useRef, useState } from 'react';

import { useAction } from '../../hooks/useAction';
import {
  cancelProviderSignIn,
  checkProviderConnection,
  connectProvider,
  disconnectProvider,
  setDefaultModel,
  signOutProvider,
  startProviderSignIn,
  type ConnectionMethod,
  type ConnectionStatus,
  type ModelRef,
  type ProviderView,
  type SignInStatus,
  type SignInView,
} from '../../services/providerService';
import { openExternalUrl } from '../../services/systemService';
import { CheckIcon, CopyIcon, ExternalIcon, KeyIcon } from '../icons';
import { StatusIndicator, type StatusTone } from '../ui/StatusIndicator';
import { KEY_NOTE, ModelList } from './ProviderRows';

type AccountMethod = Exclude<ConnectionMethod, 'api_key'>;

const METHOD_LABEL: Record<ConnectionMethod, string> = {
  api_key: 'API key',
  chatgpt_account: 'ChatGPT account',
  claude_console: 'Claude Console account',
};

const ACCOUNT_COPY: Record<AccountMethod, { button: string; hint: string; waiting: string }> = {
  chatgpt_account: {
    button: 'Continue with ChatGPT',
    hint: 'Sign in with your ChatGPT account in your browser. ReMa uses OpenAI’s Codex app on this computer, and usage counts toward your ChatGPT plan.',
    waiting: 'Finish signing in to ChatGPT in your browser. ReMa connects as soon as you approve.',
  },
  claude_console: {
    button: 'Continue with Claude Console',
    hint: 'Sign in to your Claude Console account in your browser through Anthropic’s command-line tool. Usage is billed to your Console organization at API rates, like an API key.',
    waiting:
      'Finish signing in to the Claude Console in your browser. ReMa connects as soon as you approve.',
  },
};

/** Why Claude Pro/Max sign-in is not offered (Anthropic's policy). */
const CLAUDE_POLICY_URL = 'https://code.claude.com/docs/en/legal-and-compliance';

const SIGN_IN_LABEL: Record<SignInStatus, string> = {
  connecting: 'Connecting…',
  opening_browser: 'Opening browser…',
  waiting_for_authorization: 'Waiting for authorization…',
  cancelled: 'Authentication cancelled',
  failed: 'Authentication failed',
};

const PROBLEM_LABEL: Record<Exclude<ConnectionStatus, 'connected' | 'disconnected'>, string> = {
  expired: 'Authentication expired',
  reauth_required: 'Reauthentication required',
  unavailable: 'Unavailable',
};

/** The sign-in, if one is still in progress. */
function runningSignIn(signIn: SignInView | null): SignInView | null {
  return signIn && signIn.status !== 'cancelled' && signIn.status !== 'failed' ? signIn : null;
}

function statusOf(provider: ProviderView): { tone: StatusTone; label: string } {
  const { signIn, connection, status } = provider;
  const running = runningSignIn(signIn);
  if (running) return { tone: 'pending', label: SIGN_IN_LABEL[running.status] };
  if (connection) {
    if (status === 'connected') return { tone: 'ready', label: `Connected · ${METHOD_LABEL[connection]}` };
    if (status !== 'disconnected') return { tone: 'error', label: PROBLEM_LABEL[status] };
  }
  if (signIn?.status === 'failed') return { tone: 'error', label: SIGN_IN_LABEL.failed };
  if (signIn?.status === 'cancelled') return { tone: 'idle', label: SIGN_IN_LABEL.cancelled };
  return { tone: 'idle', label: 'Disconnected' };
}

/** Asks the runtime once whether an account connection still works. */
function useConnectionCheck(provider: ProviderView) {
  const checked = useRef<string | null>(null);
  useEffect(() => {
    if (!provider.connection || provider.connection === 'api_key') return;
    const key = `${provider.id}:${provider.connection}`;
    if (checked.current === key) return;
    checked.current = key;
    // The result arrives with `providersChanged`.
    void checkProviderConnection(provider.id).catch(() => {});
  }, [provider.id, provider.connection]);
}

/**
 * OpenAI, Anthropic or Gemini: connect with an account sign-in in the
 * browser (where the provider allows it) or an API key.
 */
export function CloudProviderRow({
  provider,
  defaultModel,
}: {
  provider: ProviderView;
  defaultModel: ModelRef | null;
}) {
  const [choosing, setChoosing] = useState(false);
  const [keyForm, setKeyForm] = useState(false);
  const action = useAction();
  useConnectionCheck(provider);

  const { signIn, connection } = provider;
  const active = runningSignIn(signIn);
  const running = active !== null;
  const accountMethod =
    (provider.connectionMethods.find((m) => m !== 'api_key') as AccountMethod | undefined) ?? null;
  const connected = connection !== null;
  const problem = connected && provider.status !== 'connected';
  const showOptions = !running && (!connected || choosing);
  const status = statusOf(provider);

  const signInWithAccount = (deviceCode = false) => {
    setKeyForm(false);
    setChoosing(false);
    void action.run(() => startProviderSignIn(provider.kind, deviceCode));
  };

  const closeOptions = () => {
    setChoosing(false);
    setKeyForm(false);
    action.clearError();
  };

  return (
    <div className="provider">
      <div className="provider__row">
        <span className="provider__name">{provider.name}</span>
        <StatusIndicator tone={status.tone} label={status.label} live={running} />
        <span className="provider__spacer" />
        <div className="provider__actions">
          {running ? (
            <button
              type="button"
              className="button button--ghost"
              onClick={() => void action.run(() => cancelProviderSignIn(provider.kind))}
            >
              Cancel
            </button>
          ) : choosing ? (
            <button type="button" className="button button--ghost" onClick={closeOptions}>
              Cancel
            </button>
          ) : (
            connected && (
              <>
                {problem && connection !== 'api_key' && (
                  <button
                    type="button"
                    className="button button--secondary"
                    disabled={action.busy}
                    onClick={() => signInWithAccount()}
                  >
                    Reconnect
                  </button>
                )}
                {provider.connectionMethods.length > 1 && (
                  <button type="button" className="button button--ghost" onClick={() => setChoosing(true)}>
                    Change connection
                  </button>
                )}
                <button
                  type="button"
                  className="button button--ghost"
                  disabled={action.busy}
                  title={
                    connection === 'api_key'
                      ? 'Removes the key from your keychain'
                      : 'ReMa stops using this account; you stay signed in on this computer'
                  }
                  onClick={() => void action.run(() => disconnectProvider(provider.id))}
                >
                  Disconnect
                </button>
                {connection !== 'api_key' && (
                  <button
                    type="button"
                    className="button button--ghost"
                    disabled={action.busy}
                    title="Signs out with the provider’s app and disconnects"
                    onClick={() => void action.run(() => signOutProvider(provider.id))}
                  >
                    Sign out
                  </button>
                )}
              </>
            )
          )}
        </div>
      </div>

      {connected && provider.accountLabel && !choosing && (
        <p className="provider__account">{provider.accountLabel}</p>
      )}

      {problem && provider.statusMessage && !running && !choosing && (
        <p className="notice notice--danger" role="alert">
          {provider.statusMessage}
        </p>
      )}
      {problem && !provider.statusMessage && !running && !choosing && (
        <p className="form__hint">
          {provider.status === 'expired'
            ? 'The sign-in expired and could not be renewed. Reconnect to keep using this account.'
            : 'This computer is no longer signed in. Reconnect to keep using this account.'}
        </p>
      )}

      {active && (
        <SignInProgress provider={provider} signIn={active} onUseCode={() => signInWithAccount(true)} />
      )}

      {signIn && !running && (
        <div className={signIn.status === 'failed' ? 'notice notice--danger' : 'notice'} role="alert">
          <span className="notice__text">
            {signIn.status === 'failed'
              ? (signIn.message ?? 'The sign-in did not complete.')
              : `The ${METHOD_LABEL[signIn.method]} sign-in was cancelled.`}
          </span>
          <button
            type="button"
            className="link-button"
            onClick={() => void action.run(() => cancelProviderSignIn(provider.kind))}
          >
            Dismiss
          </button>
        </div>
      )}

      {showOptions && (
        <ConnectOptions
          provider={provider}
          accountMethod={accountMethod}
          busy={action.busy}
          keyForm={keyForm}
          onAccount={() => signInWithAccount()}
          onKey={() => {
            setKeyForm(true);
            action.clearError();
          }}
          onKeySaved={closeOptions}
          onKeyCancel={() => {
            setKeyForm(false);
            action.clearError();
          }}
        />
      )}

      {action.error && (
        <p className="form-error" role="alert">
          {action.error}
        </p>
      )}

      {connected && !choosing && provider.models.length > 0 && (
        <div className="provider__models">
          <ModelSelect provider={provider} defaultModel={defaultModel} />
          <ModelList provider={provider} />
        </div>
      )}
    </div>
  );
}

function ConnectOptions({
  provider,
  accountMethod,
  busy,
  keyForm,
  onAccount,
  onKey,
  onKeySaved,
  onKeyCancel,
}: {
  provider: ProviderView;
  accountMethod: AccountMethod | null;
  busy: boolean;
  keyForm: boolean;
  onAccount: () => void;
  onKey: () => void;
  onKeySaved: () => void;
  onKeyCancel: () => void;
}) {
  const copy = accountMethod ? ACCOUNT_COPY[accountMethod] : null;
  return (
    <div className="connect">
      {!keyForm && (
        <div className="connect__choices">
          <span className="connect__label">Connect with</span>
          <button
            type="button"
            className={copy ? 'button button--secondary' : 'button button--primary'}
            onClick={onKey}
          >
            <KeyIcon className="button__icon" />
            API key
          </button>
          {copy && (
            <>
              <span className="connect__or">or</span>
              <button type="button" className="button button--primary" disabled={busy} onClick={onAccount}>
                {copy.button}
                <ExternalIcon className="button__icon" />
              </button>
            </>
          )}
        </div>
      )}
      {!keyForm && copy && <p className="form__hint">{copy.hint}</p>}
      {!keyForm && provider.kind === 'anthropic' && (
        <p className="form__hint">
          Claude Pro and Max plans can’t be used here: Anthropic doesn’t allow third-party apps to
          offer Claude.ai sign-in.{' '}
          <button type="button" className="link-button" onClick={() => void openExternalUrl(CLAUDE_POLICY_URL)}>
            Anthropic’s policy
          </button>
        </p>
      )}
      {keyForm && <ApiKeyForm provider={provider} onSaved={onKeySaved} onCancel={onKeyCancel} />}
    </div>
  );
}

function ApiKeyForm({
  provider,
  onSaved,
  onCancel,
}: {
  provider: ProviderView;
  onSaved: () => void;
  onCancel: () => void;
}) {
  const [apiKey, setApiKey] = useState('');
  const action = useAction();

  const save = async () => {
    if (await action.run(() => connectProvider(provider.kind, apiKey))) {
      setApiKey('');
      onSaved();
    }
  };

  return (
    <form
      className="provider__form"
      onSubmit={(e) => {
        e.preventDefault();
        void save();
      }}
    >
      <div className="form__inline">
        <input
          type="password"
          className="input input--grow"
          placeholder={`${provider.name} API key`}
          aria-label={`${provider.name} API key`}
          autoComplete="off"
          spellCheck={false}
          autoFocus
          value={apiKey}
          onChange={(e) => setApiKey(e.target.value)}
        />
        <button type="submit" className="button button--primary" disabled={action.busy || !apiKey.trim()}>
          {action.busy ? 'Checking…' : 'Save'}
        </button>
        <button type="button" className="button button--ghost" onClick={onCancel}>
          Back
        </button>
      </div>
      <p className="form__hint">
        {KEY_NOTE} ReMa checks the key with {provider.name} first.
      </p>
      {action.error && (
        <p className="form-error" role="alert">
          {action.error}
        </p>
      )}
    </form>
  );
}

/** The browser sign-in in progress (and the device-code fallback). */
function SignInProgress({
  provider,
  signIn,
  onUseCode,
}: {
  provider: ProviderView;
  signIn: SignInView;
  onUseCode: () => void;
}) {
  const [copied, setCopied] = useState(false);
  const copy = signIn.method === 'api_key' ? null : ACCOUNT_COPY[signIn.method];
  const code = signIn.userCode;

  const copyCode = () => {
    if (!code) return;
    void navigator.clipboard
      .writeText(code)
      .then(() => {
        setCopied(true);
        window.setTimeout(() => setCopied(false), 1500);
      })
      .catch(() => {});
  };

  return (
    <div className="sign-in">
      <span className="spinner" aria-hidden="true" />
      <div className="sign-in__body">
        {code ? (
          <>
            <p className="sign-in__text">
              In the page that opened, sign in and enter this code to connect {provider.name}:
            </p>
            <div className="sign-in__code-row">
              <code className="sign-in__code">{code}</code>
              <button type="button" className="button button--ghost button--small" onClick={copyCode}>
                {copied ? <CheckIcon className="button__icon" /> : <CopyIcon className="button__icon" />}
                {copied ? 'Copied' : 'Copy code'}
              </button>
              {signIn.verificationUrl && (
                <button
                  type="button"
                  className="link-button"
                  onClick={() => void openExternalUrl(signIn.verificationUrl ?? '')}
                >
                  Open the page again
                </button>
              )}
            </div>
          </>
        ) : (
          <>
            <p className="sign-in__text">{copy?.waiting}</p>
            {signIn.method === 'chatgpt_account' && signIn.status === 'waiting_for_authorization' && (
              <p className="form__hint">
                Browser didn’t open, or signing in on another device?{' '}
                <button type="button" className="link-button" onClick={onUseCode}>
                  Use a code instead
                </button>
              </p>
            )}
          </>
        )}
      </div>
    </div>
  );
}

/** Which of this provider's models ReMa uses by default in chat. */
function ModelSelect({ provider, defaultModel }: { provider: ProviderView; defaultModel: ModelRef | null }) {
  const action = useAction();
  const current = defaultModel?.providerId === provider.id ? defaultModel.modelId : '';
  return (
    <label className="provider__model" title="ReMa’s default chat model">
      <span className="provider__model-label">Model</span>
      <select
        className="input"
        value={current}
        disabled={action.busy}
        onChange={(e) =>
          void action.run(() => setDefaultModel({ providerId: provider.id, modelId: e.target.value }))
        }
      >
        {current === '' && (
          <option value="" disabled>
            Choose a default model
          </option>
        )}
        {provider.models.map((m) => (
          <option key={m.modelId} value={m.modelId}>
            {m.displayName}
          </option>
        ))}
      </select>
      {action.error && <span className="form-error">{action.error}</span>}
    </label>
  );
}
