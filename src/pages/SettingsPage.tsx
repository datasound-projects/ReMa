import { useState } from 'react';

import { PlusIcon } from '../components/icons';
import { PageContainer } from '../components/layout/PageContainer';
import { CloudProviderRow, CustomEndpointRow } from '../components/settings/ProviderRows';
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
    <PageContainer title="Settings">
      <section className="settings-section" aria-labelledby="models-heading">
        <h2 id="models-heading" className="section-title">
          Models &amp; Providers
        </h2>

        {settings.state.status === 'error' && (
          <p className="form-error" role="alert">
            {settings.state.error.message}
          </p>
        )}

        <div className="panel panel--list">
          {cloud.map((provider) => (
            <CloudProviderRow key={provider.id} provider={provider} />
          ))}
        </div>

        <h3 className="subsection-title">OpenAI-compatible endpoints</h3>
        <div className="panel panel--list">
          {custom.map((provider) => (
            <CustomEndpointRow key={provider.id} provider={provider} />
          ))}
          {adding ? (
            <CustomEndpointRow onDone={() => setAdding(false)} />
          ) : (
            <div className="provider">
              <button type="button" className="button button--ghost" onClick={() => setAdding(true)}>
                <PlusIcon className="button__icon" />
                Add endpoint
              </button>
            </div>
          )}
        </div>

        <div className="default-model">
          <label className="field__label" htmlFor="default-model">
            Default model
          </label>
          <select
            id="default-model"
            className="input input--auto"
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
          <span className="form__hint">Used for new chats.</span>
        </div>
        {error && <p className="form-error">{error}</p>}
      </section>

      <section className="settings-section" aria-labelledby="about-heading">
        <h2 id="about-heading" className="section-title">
          About
        </h2>
        <p className="about">
          ReMa {status.status === 'success' ? status.data.version : ''}
          <span className="about__sep">·</span>
          {status.status === 'success' ? (
            <StatusIndicator tone="ready" label="Backend ready" />
          ) : status.status === 'error' ? (
            <StatusIndicator tone="error" label={status.error.message} />
          ) : (
            <StatusIndicator tone="pending" label="Connecting…" />
          )}
        </p>
      </section>
    </PageContainer>
  );
}
