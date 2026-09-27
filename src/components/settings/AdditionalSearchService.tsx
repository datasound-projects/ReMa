import { useEffect, useState } from 'react';

import { toApiError } from '../../services/ipc';
import {
  saveWebSearchSettings,
  testWebSearch,
  webSearchSettings,
  type ServiceKind,
  type WebSearchSettings,
} from '../../services/webSearchService';
import { FormField } from '../ui/FormField';

const SERVICES: { value: ServiceKind | ''; label: string }[] = [
  { value: '', label: 'None (not needed)' },
  { value: 'brave', label: 'Brave Search API' },
  { value: 'tavily', label: 'Tavily' },
  { value: 'searxng', label: 'SearXNG (your own instance)' },
];

type Status = { kind: 'idle' } | { kind: 'busy'; what: string } | { kind: 'ok'; text: string } | { kind: 'error'; text: string };

/**
 * Settings → Career Search → Advanced: an optional search service that adds
 * its results to ReMa's own career search. Never required.
 */
export function AdditionalSearchService() {
  const [saved, setSaved] = useState<WebSearchSettings | null>(null);
  const [service, setService] = useState<ServiceKind | ''>('');
  const [url, setUrl] = useState('');
  const [key, setKey] = useState('');
  const [status, setStatus] = useState<Status>({ kind: 'idle' });

  useEffect(() => {
    let alive = true;
    webSearchSettings()
      .then((s) => {
        if (!alive) return;
        setSaved(s);
        setService(s.service ?? '');
        setUrl(s.url ?? '');
      })
      .catch((err: unknown) => alive && setStatus({ kind: 'error', text: toApiError(err).message }));
    return () => {
      alive = false;
    };
  }, []);

  const changed =
    saved !== null && (service !== (saved.service ?? '') || url !== (saved.url ?? '') || key.trim() !== '');
  const busy = status.kind === 'busy';
  const needsKey = service === 'brave' || service === 'tavily';
  const keyStored = needsKey && saved?.service === service && saved.hasKey;

  const save = async () => {
    setStatus({ kind: 'busy', what: 'save' });
    try {
      const next = await saveWebSearchSettings({
        service: service === '' ? null : service,
        url: service === 'searxng' ? url : null,
        key: key.trim() === '' ? null : key,
      });
      setSaved(next);
      setKey('');
      setStatus({ kind: 'ok', text: next.service ? 'Saved.' : 'Removed. Career search keeps working without it.' });
    } catch (err) {
      setStatus({ kind: 'error', text: toApiError(err).message });
    }
  };

  const test = async () => {
    setStatus({ kind: 'busy', what: 'test' });
    try {
      const result = await testWebSearch();
      setStatus({
        kind: 'ok',
        text:
          result.results === 0
            ? 'The search ran but found nothing.'
            : `Works: ${result.results} results${result.titles.length > 0 ? ` (e.g. “${result.titles[0]}”)` : ''}.`,
      });
    } catch (err) {
      setStatus({ kind: 'error', text: toApiError(err).message });
    }
  };

  return (
    <div className="websearch">
      <p className="websearch__intro">
        Optional, for developers and power users: a search service you run or pay for adds its results. Career search
        works the same without one.
      </p>
      <div className="websearch__form">

        <FormField label="Search service">
          {(ids) => (
            <select
              {...ids}
              className="input input--auto"
              value={service}
              disabled={saved === null || busy}
              onChange={(e) => {
                setService(e.target.value as ServiceKind | '');
                setStatus({ kind: 'idle' });
              }}
            >
              {SERVICES.map((s) => (
                <option key={s.value} value={s.value}>
                  {s.label}
                </option>
              ))}
            </select>
          )}
        </FormField>
        {service === 'searxng' && (
          <FormField label="Address" hint="Your SearXNG instance, with the JSON format enabled (search.formats: json).">
            {(ids) => (
              <input
                {...ids}
                className="input input--mono"
                placeholder="http://localhost:8888"
                value={url}
                disabled={busy}
                onChange={(e) => setUrl(e.target.value)}
              />
            )}
          </FormField>
        )}
        {needsKey && (
          <FormField
            label="API key"
            hint={keyStored ? 'Stored in your system keychain. Leave empty to keep it.' : 'Stored in your system keychain, never in ReMa’s files.'}
          >
            {(ids) => (
              <input
                {...ids}
                className="input input--mono"
                type="password"
                autoComplete="off"
                placeholder={keyStored ? '•••••••• (unchanged)' : 'Paste the API key'}
                value={key}
                disabled={busy}
                onChange={(e) => setKey(e.target.value)}
              />
            )}
          </FormField>
        )}
        <div className="websearch__actions">
          <button type="button" className="button button--primary button--small" disabled={!changed || busy} onClick={() => void save()}>
            {busy && status.what === 'save' ? 'Saving…' : 'Save'}
          </button>
          <button
            type="button"
            className="button button--secondary button--small"
            disabled={!saved?.service || changed || busy}
            title={changed ? 'Save first' : undefined}
            onClick={() => void test()}
          >
            {busy && status.what === 'test' ? 'Searching…' : 'Test'}
          </button>
          {status.kind === 'ok' && (
            <span className="websearch__status" role="status">
              {status.text}
            </span>
          )}
        </div>
        {status.kind === 'error' && (
          <p className="form-error" role="alert">
            {status.text}
          </p>
        )}
      </div>
    </div>
  );
}
