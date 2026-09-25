import { useState } from 'react';

import { toApiError } from '../../services/ipc';
import {
  connectProvider,
  disconnectProvider,
  refreshProviderModels,
  saveCustomProvider,
  setModelEnabled,
  type ProviderView,
} from '../../services/providerService';
import { ChevronDownIcon } from '../icons';
import { StatusIndicator } from '../ui/StatusIndicator';

const KEY_NOTE = 'Stored in your system keychain, never in ReMa’s files.';

/** Runs an action, tracking busy state and a readable error. */
function useAction() {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const run = async (action: () => Promise<unknown>): Promise<boolean> => {
    setBusy(true);
    setError(null);
    try {
      await action();
      return true;
    } catch (err) {
      setError(toApiError(err).message);
      return false;
    } finally {
      setBusy(false);
    }
  };
  return { busy, error, run, clearError: () => setError(null) };
}

/** OpenAI, Anthropic or Gemini. */
export function CloudProviderRow({ provider }: { provider: ProviderView }) {
  const [connecting, setConnecting] = useState(false);
  const [apiKey, setApiKey] = useState('');
  const action = useAction();

  const connect = async () => {
    if (await action.run(() => connectProvider(provider.kind, apiKey))) {
      setConnecting(false);
      setApiKey('');
    }
  };

  return (
    <div className="provider">
      <div className="provider__row">
        <span className="provider__name">{provider.name}</span>
        {provider.configured ? (
          <StatusIndicator tone="ready" label="Connected" />
        ) : (
          <StatusIndicator tone="idle" label="Not connected" />
        )}
        <span className="provider__spacer" />
        {provider.configured ? (
          <button
            type="button"
            className="button button--ghost"
            disabled={action.busy}
            onClick={() => void action.run(() => disconnectProvider(provider.id))}
          >
            Disconnect
          </button>
        ) : (
          !connecting && (
            <button type="button" className="button button--secondary" onClick={() => setConnecting(true)}>
              Connect
            </button>
          )
        )}
      </div>

      {connecting && !provider.configured && (
        <form
          className="provider__form"
          onSubmit={(e) => {
            e.preventDefault();
            void connect();
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
            <button
              type="button"
              className="button button--ghost"
              onClick={() => {
                setConnecting(false);
                setApiKey('');
                action.clearError();
              }}
            >
              Cancel
            </button>
          </div>
          <p className="form__hint">{KEY_NOTE} ReMa checks the key with {provider.name} first.</p>
        </form>
      )}

      {action.error && (
        <p className="form-error" role="alert">
          {action.error}
        </p>
      )}
      {provider.configured && <ModelList provider={provider} />}
    </div>
  );
}

/** Which of a provider's models appear in the chat. */
function ModelList({ provider }: { provider: ProviderView }) {
  const [open, setOpen] = useState(false);
  const [filter, setFilter] = useState('');
  const action = useAction();
  const enabled = provider.models.filter((m) => m.enabled).length;
  const shown = provider.models.filter(
    (m) =>
      !filter ||
      m.modelId.toLowerCase().includes(filter.toLowerCase()) ||
      m.displayName.toLowerCase().includes(filter.toLowerCase()),
  );

  return (
    <div className="models">
      <button
        type="button"
        className="models__toggle"
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
      >
        {enabled} of {provider.models.length} models in chat
        <ChevronDownIcon className={open ? 'models__chevron models__chevron--open' : 'models__chevron'} />
      </button>
      {open && (
        <div className="models__panel">
          <div className="form__inline">
            {provider.models.length > 8 && (
              <input
                className="input input--grow"
                placeholder="Filter models"
                aria-label="Filter models"
                value={filter}
                onChange={(e) => setFilter(e.target.value)}
              />
            )}
            <button
              type="button"
              className="button button--ghost"
              disabled={action.busy}
              onClick={() => void action.run(() => refreshProviderModels(provider.id))}
            >
              {action.busy ? 'Refreshing…' : 'Refresh list'}
            </button>
          </div>
          {action.error && <p className="form-error">{action.error}</p>}
          <ul className="models__list">
            {shown.map((model) => (
              <li key={model.modelId}>
                <label className="checkbox">
                  <input
                    type="checkbox"
                    checked={model.enabled}
                    onChange={(e) =>
                      void action.run(() =>
                        setModelEnabled(
                          { providerId: provider.id, modelId: model.modelId },
                          e.target.checked,
                        ),
                      )
                    }
                  />
                  <span>{model.displayName}</span>
                  {model.displayName !== model.modelId && (
                    <span className="checkbox__hint">{model.modelId}</span>
                  )}
                </label>
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}

interface EndpointFormValue {
  name: string;
  baseUrl: string;
  model: string;
  apiKey: string;
}

/** One OpenAI-compatible endpoint, or the form to add a new one. */
export function CustomEndpointRow({
  provider,
  onDone,
}: {
  provider?: ProviderView;
  /** Called after a new endpoint was added or the form was cancelled. */
  onDone?: () => void;
}) {
  const configuredModel = provider?.configuredModel ?? '';
  const [editing, setEditing] = useState(!provider);
  const [value, setValue] = useState<EndpointFormValue>({
    name: provider?.name ?? 'Local',
    baseUrl: provider?.baseUrl ?? 'http://localhost:11434/v1',
    model: configuredModel,
    apiKey: '',
  });
  const action = useAction();

  const save = async () => {
    const ok = await action.run(() =>
      saveCustomProvider({
        id: provider?.id ?? null,
        name: value.name,
        baseUrl: value.baseUrl,
        model: value.model,
        // Empty keeps the stored key.
        apiKey: value.apiKey || null,
      }),
    );
    if (ok) {
      setEditing(false);
      setValue((v) => ({ ...v, apiKey: '' }));
      onDone?.();
    }
  };

  const removeKey = () =>
    void action.run(() =>
      saveCustomProvider({
        id: provider?.id ?? null,
        name: value.name,
        baseUrl: value.baseUrl,
        model: value.model,
        apiKey: '',
      }),
    );

  return (
    <div className="provider">
      {provider && !editing && (
        <div className="provider__row">
          <span className="provider__name">{provider.name}</span>
          <span className="provider__detail">
            {provider.baseUrl} · {configuredModel || 'no model'}
            {provider.hasCredential && ' · API key'}
          </span>
          <span className="provider__spacer" />
          <button type="button" className="button button--ghost" onClick={() => setEditing(true)}>
            Edit
          </button>
          <button
            type="button"
            className="button button--ghost"
            disabled={action.busy}
            onClick={() => void action.run(() => disconnectProvider(provider.id))}
          >
            Remove
          </button>
        </div>
      )}

      {editing && (
        <form
          className="provider__form"
          onSubmit={(e) => {
            e.preventDefault();
            void save();
          }}
        >
          <div className="form__row">
            <label className="field">
              <span className="field__label">Name</span>
              <input
                className="input"
                value={value.name}
                onChange={(e) => setValue({ ...value, name: e.target.value })}
              />
            </label>
            <label className="field field--grow">
              <span className="field__label">Base URL</span>
              <input
                className="input"
                spellCheck={false}
                placeholder="http://localhost:11434/v1"
                value={value.baseUrl}
                onChange={(e) => setValue({ ...value, baseUrl: e.target.value })}
              />
            </label>
          </div>
          <div className="form__row">
            <label className="field field--grow">
              <span className="field__label">Model</span>
              <input
                className="input"
                spellCheck={false}
                placeholder="my-model"
                value={value.model}
                onChange={(e) => setValue({ ...value, model: e.target.value })}
              />
            </label>
            <label className="field field--grow">
              <span className="field__label">API key (optional)</span>
              <input
                type="password"
                className="input"
                autoComplete="off"
                placeholder={provider?.hasCredential ? 'Saved — leave empty to keep' : 'None'}
                value={value.apiKey}
                onChange={(e) => setValue({ ...value, apiKey: e.target.value })}
              />
            </label>
          </div>
          <div className="form__inline">
            <button type="submit" className="button button--primary" disabled={action.busy}>
              {action.busy ? 'Saving…' : 'Save'}
            </button>
            <button
              type="button"
              className="button button--ghost"
              onClick={() => {
                setEditing(false);
                onDone?.();
              }}
            >
              Cancel
            </button>
            {provider?.hasCredential && (
              <button type="button" className="button button--ghost" onClick={removeKey}>
                Remove API key
              </button>
            )}
          </div>
          <p className="form__hint">Any server that implements the OpenAI Chat Completions API. {KEY_NOTE}</p>
        </form>
      )}

      {action.error && (
        <p className="form-error" role="alert">
          {action.error}
        </p>
      )}
      {provider && !editing && provider.models.length > 1 && <ModelList provider={provider} />}
    </div>
  );
}
