import { useState } from 'react';

import { PlusIcon } from '../components/icons';
import { PageContainer } from '../components/layout/PageContainer';
import { GoogleSection } from '../components/settings/GoogleSection';
import { CloudProviderRow } from '../components/settings/CloudProviderRow';
import { CustomEndpointRow } from '../components/settings/ProviderRows';
import { BrandLogo } from '../components/ui/BrandLogo';
import { StatusIndicator } from '../components/ui/StatusIndicator';
import { useAppStatus } from '../hooks/useAppStatus';
import { useModelCatalog } from '../hooks/useModelCatalog';
import { useProviderSettings } from '../hooks/useProviderSettings';
import { dataOr } from '../hooks/useAsyncData';
import { toApiError } from '../services/ipc';
import { setDefaultModel, type ModelRef } from '../services/providerService';

const modelKey = (m: ModelRef) => `${m.providerId}/${m.modelId}`;

export function SettingsPage() {
  const settings = useProviderSettings();
  const catalog = dataOr(useModelCatalog().state, null);
  const status = useAppStatus().state;
  const [adding, setAdding] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const providers = dataOr(settings.state, null)?.providers ?? [];
  const cloud = providers.filter((p) => p.kind !== 'openai_compatible');
  const custom = providers.filter((p) => p.kind === 'openai_compatible');
  const models = catalog?.models ?? [];
  const providerNames = [...new Set(models.map((m) => m.providerName))];

  return (
    <PageContainer title="Settings" subtitle="Models, connected accounts and how ReMa works for you.">
      <section className="section" aria-labelledby="models-heading">
        <div className="section__head">
          <div className="section__heading">
            <h2 id="models-heading" className="section__title">
              Models &amp; providers
            </h2>
            <p className="section__description">
              Sign in with your account in the browser, or use an API key. Sign-in runs through each
              provider’s own app on this computer, so ReMa never sees your password or tokens; keys
              stay in your system keychain.
            </p>
          </div>
        </div>

        {settings.state.status === 'error' && (
          <p className="form-error" role="alert">
            {settings.state.error.message}
          </p>
        )}

        <div className="panel panel--list">
          {cloud.map((provider) => (
            <CloudProviderRow
              key={provider.id}
              provider={provider}
              defaultModel={catalog?.defaultModel ?? null}
            />
          ))}
        </div>

      </section>

      <section className="section" aria-labelledby="endpoints-heading">
        <div className="section__head">
          <div className="section__heading">
            <h2 id="endpoints-heading" className="section__title">
              Local and compatible endpoints
            </h2>
            <p className="section__description">
              Ollama, LM Studio, vLLM or any server with the OpenAI Chat Completions API.
            </p>
          </div>
        </div>
        <div className="panel panel--list">
          {custom.map((provider) => (
            <CustomEndpointRow key={provider.id} provider={provider} />
          ))}
          {adding ? (
            <CustomEndpointRow onDone={() => setAdding(false)} />
          ) : (
            <div className="provider provider--add">
              <button type="button" className="button button--ghost entries__add" onClick={() => setAdding(true)}>
                <PlusIcon className="button__icon" />
                Add endpoint
              </button>
            </div>
          )}
        </div>
      </section>

      <section className="section" aria-labelledby="chat-heading">
        <h2 id="chat-heading" className="section__title">
          Chat
        </h2>
        <div className="panel panel--list">
          <div className="setting-row">
            <div className="setting-row__text">
              <label className="setting-row__label" htmlFor="default-model">
                Default model
              </label>
              <span className="setting-row__hint">Used for new chats and new scheduled tasks.</span>
            </div>
            <select
            id="default-model"
            className="input input--auto input--small setting-row__control"
            disabled={models.length === 0}
            value={catalog?.defaultModel ? modelKey(catalog.defaultModel) : ''}
            onChange={(e) => {
              const option = models.find((m) => modelKey(m.model) === e.target.value);
              if (option) {
                setDefaultModel(option.model).catch((err: unknown) => setError(toApiError(err).message));
              }
            }}
          >
            {models.length === 0 && <option value="">Connect a provider first</option>}
            {providerNames.map((name) => (
              <optgroup key={name} label={name}>
                {models
                  .filter((m) => m.providerName === name)
                  .map((m) => (
                    <option key={modelKey(m.model)} value={modelKey(m.model)}>
                      {m.displayName}
                    </option>
                  ))}
              </optgroup>
            ))}
          </select>
          </div>
        </div>
        {error && <p className="form-error">{error}</p>}
      </section>

      <GoogleSection />

      <section className="section" aria-labelledby="about-heading">
        <h2 id="about-heading" className="section__title">
          About
        </h2>
        <div className="panel panel--list">
          <div className="setting-row">
            <BrandLogo height={30} />
            <div className="setting-row__text">
              <span className="setting-row__label">
                Version {status.status === 'success' ? status.data.version : '…'}
              </span>
              <span className="setting-row__hint">Career UI harness. Your chats, tasks and Profile are stored in a local database on this computer.</span>
            </div>
            {status.status === 'success' ? (
              <StatusIndicator tone="ready" label="Backend ready" />
            ) : status.status === 'error' ? (
              <StatusIndicator tone="error" label={status.error.message} />
            ) : (
              <StatusIndicator tone="pending" label="Connecting…" />
            )}
          </div>
        </div>
      </section>
    </PageContainer>
  );
}
