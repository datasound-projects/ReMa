import { useCallback, useState } from 'react';

import { useAsyncData } from '../../hooks/useAsyncData';
import { toApiError } from '../../services/ipc';
import {
  clearNetworkContacts,
  getNetworkContacts,
  importNetworkContacts,
  type ContactSource,
  type ContactsSummary,
} from '../../services/networkService';
import { openExternalUrl } from '../../services/systemService';

/** Where LinkedIn members ask for their data export (Connections). */
const LINKEDIN_EXPORT_URL = 'https://www.linkedin.com/mypreferences/d/download-my-data';

const count = (n: number, one: string, many: string) => `${n.toLocaleString()} ${n === 1 ? one : many}`;

/**
 * Settings → Connectors → Professional networks: the user's own contacts
 * for Network Connect, from their LinkedIn data export and vCards (e.g.
 * saved from XING). Kept on this computer; never sent to a model.
 */
export function NetworkContactsCard() {
  const load = useCallback(() => getNetworkContacts(), []);
  const data = useAsyncData(load);
  const [summary, setSummary] = useState<ContactsSummary | null>(null);
  const [busy, setBusy] = useState<ContactSource | 'clear' | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const current = summary ?? (data.state.status === 'success' ? data.state.data : null);

  const run = async (source: ContactSource) => {
    setBusy(source);
    setError(null);
    setMessage(null);
    try {
      const result = await importNetworkContacts(source);
      if (result) {
        setSummary(result.summary);
        const read = count(result.read, 'contact', 'contacts');
        setMessage(
          [result.read > 0 ? `Imported ${read}.` : 'No contacts were imported.', ...result.problems].join(' '),
        );
      }
    } catch (err) {
      setError(toApiError(err).message);
    } finally {
      setBusy(null);
    }
  };

  const clear = async () => {
    setBusy('clear');
    setError(null);
    setMessage(null);
    try {
      setSummary(await clearNetworkContacts(null));
      setMessage('Imported contacts were removed.');
    } catch (err) {
      setError(toApiError(err).message);
    } finally {
      setBusy(null);
    }
  };

  const total = (current?.linkedin ?? 0) + (current?.vcard ?? 0);

  return (
    <div className="panel network-contacts" aria-labelledby="network-contacts-heading">
      <h4 id="network-contacts-heading" className="network-contacts__title">
        Your contacts
      </h4>
      <p className="network-contacts__text">
        Network Connect shows whom you know at the companies it finds. LinkedIn and XING do not share your contact
        list with apps, so import it yourself: ReMa keeps names, companies and positions on this computer, matches them
        itself and never sends them to a model.
      </p>
      {current && (
        <p className="network-contacts__count" role="status">
          {total === 0
            ? 'No contacts imported yet.'
            : [
                current.linkedin > 0 && count(current.linkedin, 'LinkedIn connection', 'LinkedIn connections'),
                current.vcard > 0 && count(current.vcard, 'contact from vCards', 'contacts from vCards'),
              ]
                .filter(Boolean)
                .join(' · ')}
        </p>
      )}
      <div className="network-contacts__actions">
        <button
          type="button"
          className="button button--secondary button--small"
          disabled={busy !== null}
          onClick={() => void run('linkedin_export')}
        >
          {busy === 'linkedin_export' ? 'Importing…' : 'Import LinkedIn connections…'}
        </button>
        <button
          type="button"
          className="button button--secondary button--small"
          disabled={busy !== null}
          onClick={() => void run('vcard')}
        >
          {busy === 'vcard' ? 'Importing…' : 'Import XING or other contacts (vCard)…'}
        </button>
        {total > 0 && (
          <button type="button" className="button button--ghost button--small" disabled={busy !== null} onClick={() => void clear()}>
            Remove imported contacts
          </button>
        )}
      </div>
      <p className="form__hint network-contacts__hint">
        LinkedIn: request your data with “Connections” selected at{' '}
        <button
          type="button"
          className="link-button"
          onClick={() => void openExternalUrl(LINKEDIN_EXPORT_URL).catch(() => {})}
        >
          linkedin.com → Get a copy of your data
        </button>
        , then pick the ZIP (or Connections.csv) LinkedIn sends you. A newer export replaces the earlier one. XING: save
        contacts as vCards from their profiles, or export them from your address book.
      </p>
      {message && <p className="network-contacts__message">{message}</p>}
      {error && (
        <p className="form-error" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}
