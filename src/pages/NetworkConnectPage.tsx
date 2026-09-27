import { useCallback, useEffect, useRef, useState } from 'react';

import { useNavigation } from '../app/navigation';
import {
  AlertIcon,
  CheckIcon,
  ChevronDownIcon,
  ChevronRightIcon,
  ExternalIcon,
  NetworkIcon,
  SearchIcon,
} from '../components/icons';
import { PageContainer } from '../components/layout/PageContainer';
import { ConnectorLogo } from '../components/settings/ConnectorIcons';
import { TaskDialog } from '../components/tasks/TaskDialog';
import { Dialog } from '../components/ui/Dialog';
import { LoadingState } from '../components/ui/EmptyState';
import { StatusIndicator } from '../components/ui/StatusIndicator';
import { useAction } from '../hooks/useAction';
import { dataOr } from '../hooks/useAsyncData';
import { useModelCatalog } from '../hooks/useModelCatalog';
import { useNetworkCapabilities } from '../hooks/useNetwork';
import { useSystemTimezone } from '../hooks/useTasks';
import { formatDate, formatDateTime } from '../lib/format';
import { cancelConnectorSignIn, connectConnector } from '../services/connectorService';
import { backendEvents, subscribe } from '../services/events';
import { toApiError } from '../services/ipc';
import {
  cancelNetworkResearch,
  getLastNetworkResult,
  researchNetwork,
  type Company,
  type Confidence,
  type Connection,
  type ConnectionsOutcome,
  type Evidence,
  type JobRef,
  type NetworkResult,
  type Person,
  type ProviderCapabilities,
  type RelevanceType,
  type Stage,
} from '../services/networkService';
import { openExternalUrl } from '../services/systemService';

const EXAMPLES = [
  'Find fintech companies in Vienna hiring Product Managers, then find the Head of Product or recruiting contact for each.',
  'Which Vienna companies currently appear to be building AI teams based on open roles?',
  'Find 50 renewable-energy companies in Vienna and show their company pages.',
  'Do I know anyone at Bitpanda?',
];

const RELEVANCE_LABELS: Record<RelevanceType, string> = {
  hiring_manager: 'Hiring manager (stated in the posting)',
  named_recruiter: 'Named contact on the posting',
  stated_manager: 'Manager of the role (stated in the posting)',
  department_leader: 'Likely relevant department leader',
  team_lead: 'Possible team lead',
  recruiter: 'Relevant recruiter',
  executive: 'Company leadership',
  relevant_contact: 'Likely relevant contact',
};

const STAGE_LABELS: Record<Stage, string> = {
  companies: 'Companies',
  jobs: 'Jobs',
  people: 'People',
  connections: 'Connections',
};

const CONFIDENCE_LABELS: Record<Confidence, string> = {
  high: 'High',
  medium: 'Medium',
  low: 'Low — not confirmed',
};

const SUPPORTS_LABELS: Record<Evidence['supports'], string> = {
  company_identity: 'Company',
  company_website: 'Website',
  company_location: 'Location',
  company_industry: 'Industry',
  company_size: 'Size',
  company_hiring: 'Current openings',
  job_is_open: 'The job is open',
  current_title: 'Current title',
  person_relevance: 'Relevance',
  named_on_posting: 'Named on the posting',
  profile_link: 'Profile link',
  relationship: 'Your connection',
};

/** External links always open in the system browser (NC §31). */
function open(url: string | null | undefined) {
  if (url) void openExternalUrl(url).catch(() => {});
}

function newRunId() {
  return `nc-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
}

/**
 * Network Connect: professional research across companies, their jobs and
 * the people behind them — and, only where a provider grants it, the
 * user's own connections.
 */
export function NetworkConnectPage() {
  const capabilities = useNetworkCapabilities();
  const providers = dataOr(capabilities.state, [] as ProviderCapabilities[]);
  const [query, setQuery] = useState('');
  const [result, setResult] = useState<NetworkResult | null>(null);
  const [runId, setRunId] = useState<string | null>(null);
  const [status, setStatus] = useState<string>('');
  const [error, setError] = useState<string | null>(null);
  const [trackCompany, setTrackCompany] = useState<string | null>(null);
  const catalog = dataOr(useModelCatalog().state, null);
  const timezone = dataOr(useSystemTimezone().state, 'UTC');
  const runRef = useRef<string | null>(null);

  // The last result of this session (nothing is kept on disk).
  useEffect(() => {
    let active = true;
    void getLastNetworkResult()
      .then((last) => {
        if (active && last && !runRef.current) {
          setResult(last);
          setQuery(last.query);
        }
      })
      .catch(() => {});
    return () => {
      active = false;
    };
  }, []);

  useEffect(
    () =>
      subscribe(backendEvents.networkProgress, (event) => {
        if (event.runId === runRef.current) setStatus(event.text);
      }),
    [],
  );

  const search = useCallback(
    async (text: string) => {
      const trimmed = text.trim();
      if (!trimmed || runRef.current) return;
      const id = newRunId();
      runRef.current = id;
      setRunId(id);
      setStatus('Planning the research…');
      setError(null);
      try {
        const found = await researchNetwork({ runId: id, query: trimmed, jobUrl: null, company: null });
        setResult(found);
      } catch (err) {
        setError(toApiError(err).message);
      } finally {
        runRef.current = null;
        setRunId(null);
        setStatus('');
      }
    },
    [],
  );

  return (
    <PageContainer
      title="Network Connect"
      subtitle="Connect your professional network to research companies, jobs and the people behind them."
      width="wide"
    >
      <div className="nc-providers" aria-label="Professional networks">
        {capabilities.state.status === 'loading' && <LoadingState />}
        {providers.map((p) => (
          <ProviderCard key={p.provider} caps={p} />
        ))}
      </div>

      <form
        className="nc-search"
        onSubmit={(event) => {
          event.preventDefault();
          void search(query);
        }}
      >
        <label htmlFor="nc-query" className="sr-only">
          Research request
        </label>
        <textarea
          id="nc-query"
          className="input input--textarea nc-search__input"
          rows={2}
          placeholder="Find fintech companies in Vienna hiring AI roles and the relevant hiring contacts…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && !e.shiftKey) {
              e.preventDefault();
              void search(query);
            }
          }}
          disabled={runId !== null}
        />
        <div className="nc-search__actions">
          {runId !== null ? (
            <>
              <span className="nc-search__status" role="status">
                <span className="spinner" aria-hidden="true" />
                {status || 'Researching…'}
              </span>
              <button
                type="button"
                className="button button--secondary"
                onClick={() => void cancelNetworkResearch(runId).catch(() => {})}
              >
                Stop
              </button>
            </>
          ) : (
            <button type="submit" className="button button--primary" disabled={!query.trim()}>
              <SearchIcon className="button__icon" />
              Search
            </button>
          )}
        </div>
      </form>
      {!result && runId === null && (
        <div className="nc-examples" aria-label="Examples">
          {EXAMPLES.map((example) => (
            <button
              key={example}
              type="button"
              className="nc-example"
              onClick={() => {
                setQuery(example);
                void search(example);
              }}
            >
              {example}
            </button>
          ))}
        </div>
      )}
      {error && (
        <p className="form-error" role="alert">
          {error}
        </p>
      )}
      {result && <ResultView result={result} onTrack={setTrackCompany} />}
      {trackCompany !== null && (
        <TaskDialog
          catalog={catalog}
          timezone={timezone}
          initialPrompt={`Track ${trackCompany}: find its current open roles and the most relevant hiring-side contacts at ${trackCompany}.`}
          initialModel={catalog?.defaultModel ?? null}
          onClose={() => setTrackCompany(null)}
          onSaved={() => setTrackCompany(null)}
        />
      )}
    </PageContainer>
  );
}

/** One professional network: what it lets ReMa do (NC §7, §52). */
function ProviderCard({ caps }: { caps: ProviderCapabilities }) {
  const { navigate } = useNavigation();
  const action = useAction();
  const [advanced, setAdvanced] = useState(false);
  const id = caps.provider === 'xing' ? 'xing' : 'linkedin';
  const connect = () =>
    void action.run(() => connectConnector(id)).then((ok) => {
      if (!ok) void cancelConnectorSignIn(caps.provider).catch(() => {});
    });
  return (
    <section className="nc-provider" aria-label={caps.name}>
      <div className="nc-provider__head">
        <ConnectorLogo id={id} size={32} />
        <div className="nc-provider__title">
          <span className="nc-provider__name">{caps.name}</span>
          <ProviderStatus caps={caps} />
        </div>
        {caps.access === 'not_connected' && (
          <button type="button" className="button button--primary button--small" disabled={action.busy} onClick={connect}>
            {action.busy ? 'Waiting for sign-in…' : 'Connect'}
          </button>
        )}
        {caps.access === 'reconnect_needed' && (
          <button type="button" className="button button--secondary button--small" disabled={action.busy} onClick={connect}>
            Reconnect
          </button>
        )}
        {caps.access === 'connected' && caps.grantAvailable && (
          <button
            type="button"
            className="button button--secondary button--small"
            disabled={action.busy}
            title="Sign in to LinkedIn again to let ReMa read your first-degree connections."
            onClick={connect}
          >
            {action.busy ? 'Waiting for sign-in…' : 'Grant connection access'}
          </button>
        )}
      </div>
      <p className="nc-provider__summary">{caps.summary}</p>
      {caps.available.length > 0 && (
        <div className="nc-provider__list">
          <span className="nc-provider__list-title">Available to ReMa</span>
          <ul>
            {caps.available.map((c) => (
              <li key={c.capability} className="is-available">
                <CheckIcon className="nc-provider__icon" aria-hidden="true" />
                {c.label}
              </li>
            ))}
          </ul>
        </div>
      )}
      {caps.access !== 'not_available' && (
        <div className="nc-provider__list">
          <span className="nc-provider__list-title">Not available to this ReMa integration</span>
          <ul>
            {caps.unavailable
              .filter((c) => caps.access === 'connected' || c.capability === 'read_first_degree_connections')
              .map((c) => (
                <li key={c.capability} className="is-unavailable" title={c.reason}>
                  <span className="nc-provider__dash" aria-hidden="true">
                    –
                  </span>
                  {c.label}
                </li>
              ))}
          </ul>
        </div>
      )}
      {action.error && (
        <p className="form-error" role="alert">
          {action.error}
        </p>
      )}
      <div className="nc-provider__foot">
        {caps.provider === 'linkedin' && (
          <button type="button" className="link-button" onClick={() => navigate({ page: 'settings', focus: 'connectors' })}>
            Manage in Settings → Connectors
          </button>
        )}
        {caps.grantedScopes.length > 0 && (
          <button
            type="button"
            className="link-button"
            aria-expanded={advanced}
            onClick={() => setAdvanced(!advanced)}
          >
            {advanced ? 'Hide advanced details' : 'Advanced details'}
          </button>
        )}
      </div>
      {advanced && (
        <p className="nc-provider__scopes">
          Granted permissions: <code>{caps.grantedScopes.join(' ')}</code>
        </p>
      )}
    </section>
  );
}

function ProviderStatus({ caps }: { caps: ProviderCapabilities }) {
  switch (caps.access) {
    case 'connected':
      return <StatusIndicator tone="ready" label={caps.accountName ? `Connected as ${caps.accountName}` : 'Connected'} />;
    case 'reconnect_needed':
      return <StatusIndicator tone="error" label="Reconnect needed" />;
    case 'not_available':
      return <StatusIndicator tone="idle" label="Not available" />;
    default:
      return <StatusIndicator tone="idle" label="Not connected" />;
  }
}

function plural(n: number, one: string, many: string) {
  return `${n} ${n === 1 ? one : many}`;
}

const STATUS_BADGES: Record<NetworkResult['status'], { label: string; className: string } | null> = {
  complete: null,
  partial: { label: 'Partial: some sources could not be searched', className: 'badge badge--warning' },
  no_verified_matches: { label: 'No verified matches', className: 'badge' },
  failed: { label: 'No source could be searched', className: 'badge badge--danger' },
  cancelled: { label: 'Stopped', className: 'badge' },
};

/** A research answer: summary, one table, connections, notes, sources. */
function ResultView({ result, onTrack }: { result: NetworkResult; onTrack: (company: string) => void }) {
  const [openRow, setOpenRow] = useState<number | null>(null);
  const [showSearched, setShowSearched] = useState(false);
  const stages = result.criteria.stages;
  const peopleMode = stages.includes('people') && result.people.length > 0;
  const badge = STATUS_BADGES[result.status];
  const criteria = [
    ...result.criteria.locations,
    ...result.criteria.industries,
    result.criteria.companySize,
    ...result.criteria.technologies.map((t) => `Technology: ${t}`),
    ...result.criteria.roles.map((r) => `Hiring: ${r}`),
    result.criteria.minOpenings !== null ? `At least ${result.criteria.minOpenings} openings` : null,
    result.criteria.postedWithinDays !== null ? `Posted within ${result.criteria.postedWithinDays} days` : null,
    ...result.criteria.people.map((p) => `People: ${p}`),
    result.criteria.targetCompany,
    result.criteria.relationships ? 'Your connections' : null,
  ].filter((c): c is string => Boolean(c));

  const rowData = result.rows.map((row) => ({
    company: result.companies.find((c) => c.id === row.companyId) ?? null,
    job: result.jobs.find((j) => j.id === row.jobId) ?? null,
    person: result.people.find((p) => p.id === row.personId) ?? null,
  }));
  const opened = openRow !== null ? rowData[openRow] : null;

  return (
    <section className="nc-result" aria-label="Results">
      <div className="nc-result__summary">
        <span className="nc-result__counts">
          {[
            plural(result.companies.length, 'company', 'companies'),
            stages.includes('jobs') ? plural(result.jobs.length, 'open role', 'open roles') : null,
            stages.includes('people') ? plural(result.people.length, 'relevant person', 'relevant people') : null,
          ]
            .filter(Boolean)
            .join(' · ')}
        </span>
        <span className="nc-result__time">Retrieved {formatDateTime(result.retrievedAt)}</span>
        {badge && <span className={badge.className}>{badge.label}</span>}
      </div>
      {criteria.length > 0 && (
        <ul className="nc-criteria" aria-label="What ReMa searched for">
          {criteria.map((c) => (
            <li key={c} className="nc-criteria__chip">
              {c}
            </li>
          ))}
        </ul>
      )}

      {result.status === 'no_verified_matches' && (
        <p className="nc-result__empty">
          ReMa found no verified matches in the sources it searched. It does not fill the gap from memory.
        </p>
      )}

      {rowData.length > 0 && (
        <div className="table-wrap nc-table-wrap">
          <table className="data-table nc-table">
            <thead>
              {peopleMode ? (
                <tr>
                  <th scope="col">Company</th>
                  <th scope="col">Open role</th>
                  <th scope="col">Relevant person</th>
                  <th scope="col">Why relevant</th>
                  <th scope="col">Confidence</th>
                  <th scope="col">Links</th>
                </tr>
              ) : (
                <tr>
                  <th scope="col">Company</th>
                  <th scope="col">Location</th>
                  <th scope="col">Industry</th>
                  <th scope="col">Size</th>
                  <th scope="col">Why it matched</th>
                  <th scope="col">Links</th>
                </tr>
              )}
            </thead>
            <tbody>
              {rowData.map((row, index) =>
                row.company ? (
                  <tr key={index} className="nc-table__row" onClick={() => setOpenRow(index)}>
                    <td className="nc-table__company">
                      <button
                        type="button"
                        className="link-button"
                        onClick={(e) => {
                          e.stopPropagation();
                          setOpenRow(index);
                        }}
                      >
                        {row.company.name}
                      </button>
                    </td>
                    {peopleMode ? (
                      <PersonCells company={row.company} job={row.job} person={row.person} />
                    ) : (
                      <CompanyCells company={row.company} job={row.job} />
                    )}
                  </tr>
                ) : null,
              )}
            </tbody>
          </table>
        </div>
      )}

      <ConnectionsPanel outcome={result.connectionsOutcome} connections={result.connections} />

      {result.notes.length > 0 && (
        <ul className="nc-notes">
          {result.notes.map((note) => (
            <li key={note}>{note}</li>
          ))}
        </ul>
      )}

      {result.stages.length > 0 && (
        <div className="nc-searched">
          <button
            type="button"
            className="button button--ghost button--small"
            aria-expanded={showSearched}
            onClick={() => setShowSearched(!showSearched)}
          >
            {showSearched ? <ChevronDownIcon className="button__icon" /> : <ChevronRightIcon className="button__icon" />}
            What ReMa searched
          </button>
          {showSearched && (
            <dl className="nc-searched__list">
              {result.stages.map((s) => (
                <div key={s.stage} className="nc-searched__stage">
                  <dt>{STAGE_LABELS[s.stage]}</dt>
                  <dd>
                    {s.summary}
                    {s.sources.length > 0 && <span className="nc-searched__sources"> · {s.sources.join(', ')}</span>}
                    {s.failed.map((f) => (
                      <span key={f} className="nc-searched__failed">
                        <AlertIcon className="nc-searched__icon" aria-hidden="true" />
                        {f}
                      </span>
                    ))}
                  </dd>
                </div>
              ))}
            </dl>
          )}
        </div>
      )}

      {opened?.company && (
        <DetailDialog
          company={opened.company}
          job={opened.job}
          person={opened.person}
          connections={result.connections.filter((c) => c.companyId === opened.company?.id)}
          onClose={() => setOpenRow(null)}
          onTrack={() => {
            const name = opened.company?.name ?? '';
            setOpenRow(null);
            onTrack(name);
          }}
        />
      )}
    </section>
  );
}

function companyLinks(company: Company) {
  return [
    company.website ? { label: 'Website', url: company.website } : null,
    company.linkedinUrl ? { label: 'LinkedIn', url: company.linkedinUrl } : null,
    company.xingUrl ? { label: 'XING', url: company.xingUrl } : null,
  ].filter((l): l is { label: string; url: string } => l !== null);
}

function personLinks(person: Person) {
  return [
    person.linkedinUrl ? { label: 'LinkedIn', url: person.linkedinUrl } : null,
    person.xingUrl ? { label: 'XING', url: person.xingUrl } : null,
    person.otherUrl ? { label: 'Profile', url: person.otherUrl } : null,
  ].filter((l): l is { label: string; url: string } => l !== null);
}

function Links({ links }: { links: { label: string; url: string }[] }) {
  if (links.length === 0) return <span className="nc-muted">—</span>;
  return (
    <span className="nc-links">
      {links.map((l) => (
        <button
          key={l.label + l.url}
          type="button"
          className="link-button nc-links__link"
          onClick={(e) => {
            e.stopPropagation();
            open(l.url);
          }}
        >
          {l.label}
          <ExternalIcon className="nc-links__icon" aria-hidden="true" />
        </button>
      ))}
    </span>
  );
}

function PersonCells({ company, job, person }: { company: Company; job: JobRef | null; person: Person | null }) {
  const links = [...companyLinks(company).slice(0, 1), ...(job ? [{ label: 'Job', url: job.url }] : [])];
  if (person) links.push(...personLinks(person));
  return (
    <>
      <td>{job?.title ?? <span className="nc-muted">—</span>}</td>
      <td>
        {person ? (
          <>
            <span className="nc-person__name">{person.name}</span>
            {person.title && <span className="nc-person__title">{person.title}</span>}
            {person.relationship && <span className="badge badge--brand nc-person__rel">{person.relationship.label}</span>}
          </>
        ) : (
          <span className="nc-muted">No relevant person verified</span>
        )}
      </td>
      <td className="nc-table__why">
        {person ? (
          <>
            <span className="nc-why__type">{RELEVANCE_LABELS[person.relevance]}</span>
            <span className="nc-why__reason">{person.relevanceReason}</span>
          </>
        ) : (
          <span className="nc-muted">No permitted source named one</span>
        )}
      </td>
      <td>
        {person ? (
          <span className={`nc-confidence nc-confidence--${person.confidence}`}>{CONFIDENCE_LABELS[person.confidence]}</span>
        ) : (
          <span className="nc-muted">—</span>
        )}
      </td>
      <td>
        <Links links={links} />
      </td>
    </>
  );
}

function CompanyCells({ company, job }: { company: Company; job: JobRef | null }) {
  const links = companyLinks(company);
  if (job) links.push({ label: 'Job', url: job.url });
  return (
    <>
      <td>{company.locations.join(' / ') || <span className="nc-muted">Unknown</span>}</td>
      <td>{company.industry ?? <span className="nc-muted">Unknown</span>}</td>
      <td>{company.size ?? <span className="nc-muted">Unknown</span>}</td>
      <td className="nc-table__why">
        {company.matchedBecause.join('; ')}
        {company.unverified.length > 0 && (
          <span className="nc-unverified">Not verified: {company.unverified.join(', ')}</span>
        )}
      </td>
      <td>
        <Links links={links} />
      </td>
    </>
  );
}

/** What the connection check did (NC §22: never "no connections" unchecked). */
function ConnectionsPanel({ outcome, connections }: { outcome: ConnectionsOutcome; connections: Connection[] }) {
  if (outcome.state === 'not_requested') return null;
  return (
    <div className={`nc-connections nc-connections--${outcome.state}`} role="note">
      <NetworkIcon className="nc-connections__icon" aria-hidden="true" />
      <div className="nc-connections__body">
        <span className="nc-connections__title">Your connections</span>
        {outcome.state === 'unavailable' && <p>{outcome.reason}</p>}
        {outcome.state === 'failed' && <p>Your connections could not be checked: {outcome.reason}</p>}
        {outcome.state === 'checked' && outcome.matched === 0 && (
          <p>
            ReMa checked your {plural(outcome.checked, 'first-degree connection', 'first-degree connections')} on
            LinkedIn: none of them lists one of these companies in their headline.
          </p>
        )}
        {outcome.state === 'checked' && connections.length > 0 && (
          <>
            <p className="nc-connections__hint">
              From LinkedIn, shown for this session only; not saved in chats or run history.
            </p>
            <ul className="nc-connections__list">
              {connections.map((c) => (
                <li key={c.name + (c.companyId ?? '')}>
                  <span className="nc-person__name">{c.name}</span>
                  {c.headline && <span className="nc-person__title">{c.headline}</span>}
                  <span className="badge badge--brand">{c.relationship.label}</span>
                  {c.profileUrl && <Links links={[{ label: 'Open profile', url: c.profileUrl }]} />}
                </li>
              ))}
            </ul>
          </>
        )}
      </div>
    </div>
  );
}

function EvidenceList({ evidence }: { evidence: Evidence[] }) {
  if (evidence.length === 0) return null;
  return (
    <ul className="nc-evidence">
      {evidence.map((e, i) => (
        <li key={i} className={e.checked ? '' : 'is-unchecked'}>
          <span className="nc-evidence__supports">{SUPPORTS_LABELS[e.supports]}</span>
          <span className="nc-evidence__source">
            {e.url ? (
              <button type="button" className="link-button" onClick={() => open(e.url)}>
                {e.title ?? e.sourceName}
              </button>
            ) : (
              (e.title ?? e.sourceName)
            )}
            {' · '}
            {e.sourceName} · retrieved {formatDateTime(e.retrievedAt)}
            {!e.checked && ' · reported by a search, not checked'}
          </span>
          {e.excerpt && <span className="nc-evidence__excerpt">“{e.excerpt}”</span>}
        </li>
      ))}
    </ul>
  );
}

/** The drill-down for one row (NC §30). */
function DetailDialog({
  company,
  job,
  person,
  connections,
  onClose,
  onTrack,
}: {
  company: Company;
  job: JobRef | null;
  person: Person | null;
  connections: Connection[];
  onClose: () => void;
  onTrack: () => void;
}) {
  return (
    <Dialog
      title={person?.name ?? company.name}
      size="wide"
      onClose={onClose}
      actions={
        <>
          <button type="button" className="button button--secondary" onClick={onTrack}>
            Track this company…
          </button>
          <button type="button" className="button button--primary" onClick={onClose}>
            Close
          </button>
        </>
      }
    >
      <div className="nc-detail">
        {person && (
          <section className="nc-detail__block" aria-label="Person">
            {/* The dialog's title already names the person. */}
            <p className="nc-detail__meta">
              {[person.title, person.companyName, person.location].filter(Boolean).join(' · ')}
            </p>
            <p>
              <strong>{RELEVANCE_LABELS[person.relevance]}</strong> — {person.relevanceReason}
            </p>
            <p>
              Confidence:{' '}
              <span className={`nc-confidence nc-confidence--${person.confidence}`}>
                {CONFIDENCE_LABELS[person.confidence]}
              </span>
            </p>
            {person.caveat && <p className="nc-muted">{person.caveat}</p>}
            {person.relationship && (
              <p>
                <span className="badge badge--brand">{person.relationship.label}</span> (from LinkedIn, this session only)
              </p>
            )}
            {personLinks(person).length > 0 && <Links links={personLinks(person)} />}
            <EvidenceList evidence={person.evidence} />
          </section>
        )}
        {job && (
          <section className="nc-detail__block" aria-label="Job">
            <h3 className="nc-detail__title">{job.title}</h3>
            <p className="nc-detail__meta">
              {[job.location, job.workMode, job.postedAt !== null ? `posted ${formatDate(job.postedAt)}` : null, job.status]
                .filter(Boolean)
                .join(' · ')}
            </p>
            {job.notes.length > 0 && <p className="nc-muted">{job.notes.join(' · ')}</p>}
            <Links links={[{ label: `Open on ${job.source}`, url: job.url }]} />
          </section>
        )}
        <section className="nc-detail__block" aria-label="Company">
          {person && <h3 className="nc-detail__title">{company.name}</h3>}
          <dl className="nc-detail__facts">
            <dt>Location</dt>
            <dd>{company.locations.join(' / ') || 'Unknown'}</dd>
            <dt>Industry</dt>
            <dd>{company.industry ?? 'Unknown'}</dd>
            <dt>Size</dt>
            <dd>{company.size ?? 'Unknown'}</dd>
            {company.relevantOpenings > 0 && (
              <>
                <dt>Openings found</dt>
                <dd>{plural(company.relevantOpenings, 'relevant opening', 'relevant openings')} in this search</dd>
              </>
            )}
          </dl>
          {company.matchedBecause.length > 0 && <p>Why it matched: {company.matchedBecause.join('; ')}</p>}
          {company.unverified.length > 0 && <p className="nc-unverified">Not verified: {company.unverified.join(', ')}</p>}
          <Links links={companyLinks(company)} />
          {connections.length > 0 && (
            <p>
              {plural(connections.length, 'first-degree connection', 'first-degree connections')} at {company.name} (from
              LinkedIn, this session only).
            </p>
          )}
          <EvidenceList evidence={company.evidence} />
        </section>
      </div>
    </Dialog>
  );
}
