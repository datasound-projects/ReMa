import { Fragment, useEffect, useState } from 'react';

import { useAction } from '../../hooks/useAction';
import { useBusinessRun } from '../../hooks/useBusiness';
import { formatDateTime } from '../../lib/format';
import {
  findClients,
  getLastBusinessResults,
  saveProspect,
  type ClientCriteria,
  type ClientProspect,
  type ClientResults,
  type Offer,
} from '../../services/businessService';
import { ChevronDownIcon, ChevronRightIcon, SearchIcon } from '../icons';
import { ChipInput } from '../profile/EntryList';
import { Switch } from '../ui/Switch';
import { Chips, FitCell, Links, LocationsField, RunProgress, SearchedDetails, SourceNotes, StatusBadge } from './common';
import { EMPTY_LOCATIONS, isEmptyLocations } from './helpers';
import { AUTHORITY_LABELS, fitLabel } from './labels';

const EXAMPLES = [
  'Find Austrian manufacturing companies that could plausibly use this service, and identify relevant technical or operations decision-makers.',
  'Find companies in Vienna that match this product’s integrations and use cases, even when they have no current vacancies.',
  'Find Austrian and German companies where its internal knowledge-search features could be relevant.',
];

const EMPTY_CRITERIA: ClientCriteria = {
  locations: EMPTY_LOCATIONS,
  industries: [],
  minEmployees: null,
  maxEmployees: null,
  exclusions: [],
  limit: null,
};

function describeCriteria(c: ClientCriteria): string[] {
  const out: string[] = [];
  const l = c.locations;
  const places = [...l.cities, ...l.regions, ...l.countries];
  if (places.length) out.push(places.join(' or '));
  if (l.radiusKm !== null) out.push(`within ${l.radiusKm} km (approximate)`);
  if (c.industries.length) out.push(c.industries.join(' or '));
  if (c.minEmployees !== null || c.maxEmployees !== null) {
    out.push(
      c.maxEmployees === null
        ? `${c.minEmployees}+ employees`
        : `${c.minEmployees ?? 1}–${c.maxEmployees} employees`,
    );
  }
  for (const e of c.exclusions) out.push(`Excluding ${e}`);
  if (c.limit !== null) out.push(`Up to ${c.limit} companies`);
  return out;
}

/**
 * Find Clients (B8): organizations that may buy the selected reviewed
 * offer. A prospect is not a customer, a warm lead or an open procurement.
 */
export function FindClients({
  offer,
  onEditOffer,
  onOpenOpportunity,
}: {
  offer: Offer | null;
  onEditOffer: () => void;
  onOpenOpportunity: (id: string) => void;
}) {
  const [query, setQuery] = useState('');
  const [criteria, setCriteria] = useState<ClientCriteria>(EMPTY_CRITERIA);
  const [showCriteria, setShowCriteria] = useState(false);
  const [findPeople, setFindPeople] = useState(true);
  const [results, setResults] = useState<ClientResults | null>(null);
  const run = useBusinessRun();

  useEffect(() => {
    let active = true;
    void getLastBusinessResults()
      .then((last) => {
        if (active && last.clients) setResults((current) => current ?? last.clients);
      })
      .catch(() => {});
    return () => {
      active = false;
    };
  }, []);

  const reviewed = offer !== null && offer.currentVersion !== null && !offer.archived;

  const search = (text: string) => {
    if (!offer || !reviewed || run.running) return;
    void run.start(async (runId) => {
      const found = await findClients({
        runId,
        offerId: offer.id,
        offerVersion: offer.currentVersion,
        query: text.trim(),
        criteria,
        findPeople,
      });
      setResults(found);
      return found;
    });
  };

  const markSaved = (companyKey: string, opportunityId: string) =>
    setResults((r) => {
      if (!r) return r;
      const mark = (list: ClientProspect[]) =>
        list.map((p) => (p.companyKey === companyKey ? { ...p, opportunityId } : p));
      return { ...r, confirmed: mark(r.confirmed), needsVerification: mark(r.needsVerification), excluded: mark(r.excluded) };
    });

  return (
    <div className="biz-view">
      {!reviewed && (
        <div className="notice notice--warning" role="note">
          <span>
            {offer === null
              ? 'Describe what you offer first: Find Clients searches for buyers of a reviewed offer.'
              : offer.archived
                ? `“${offer.name}” is archived. Restore it or choose another offer.`
                : `“${offer.name}” has not been reviewed yet. Review its claims and save a version first.`}
          </span>
          <button type="button" className="button button--secondary button--small" onClick={onEditOffer}>
            {offer === null ? 'Describe an offer' : 'Open offer'}
          </button>
        </div>
      )}

      <form
        className="biz-search"
        onSubmit={(e) => {
          e.preventDefault();
          search(query);
        }}
      >
        <label htmlFor="biz-clients-query" className="sr-only">
          Which clients to look for
        </label>
        <textarea
          id="biz-clients-query"
          className="input input--textarea biz-search__input"
          rows={2}
          placeholder="Find manufacturing companies in Austria that could plausibly use this offer…"
          value={query}
          disabled={run.running}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && !e.shiftKey) {
              e.preventDefault();
              search(query);
            }
          }}
        />
        <div className="biz-search__bar">
          <button
            type="button"
            className="button button--ghost button--small"
            aria-expanded={showCriteria}
            onClick={() => setShowCriteria(!showCriteria)}
          >
            {showCriteria ? <ChevronDownIcon className="button__icon" /> : <ChevronRightIcon className="button__icon" />}
            Criteria
            {(!isEmptyLocations(criteria.locations) || criteria.industries.length > 0) && (
              <span className="badge badge--brand">set</span>
            )}
          </button>
          <Switch checked={findPeople} onChange={setFindPeople}>
            <span className="biz-switch-label">Identify buyer roles and professional contacts</span>
          </Switch>
          <span className="biz-search__spacer" />
          {run.running ? (
            <RunProgress status={run.status} fallback="Reviewing offer…" onStop={run.stop} />
          ) : (
            <button type="submit" className="button button--primary" disabled={!reviewed}>
              <SearchIcon className="button__icon" />
              Find clients
            </button>
          )}
        </div>
        {showCriteria && (
          <div className="biz-criteria">
            <p className="form__hint">
              Empty fields come from your request, then the offer’s availability, then your Business Profile. Places
              in one field mean “or”; different fields must all match.
            </p>
            <LocationsField value={criteria.locations} onChange={(locations) => setCriteria({ ...criteria, locations })} />
            <div className="biz-form-grid">
              <div className="field">
                <span className="field__label">Industries</span>
                <ChipInput
                  label="Industries"
                  placeholder="Manufacturing, logistics…"
                  values={criteria.industries}
                  onChange={(industries) => setCriteria({ ...criteria, industries })}
                />
              </div>
              <div className="field">
                <span className="field__label">Exclude</span>
                <ChipInput
                  label="Exclude"
                  placeholder="Companies or kinds to leave out"
                  values={criteria.exclusions}
                  onChange={(exclusions) => setCriteria({ ...criteria, exclusions })}
                />
              </div>
              <label className="field">
                <span className="field__label">Employees from</span>
                <input
                  className="input input--number"
                  type="number"
                  min={1}
                  value={criteria.minEmployees ?? ''}
                  onChange={(e) => setCriteria({ ...criteria, minEmployees: e.target.value ? Number(e.target.value) : null })}
                />
              </label>
              <label className="field">
                <span className="field__label">Employees up to</span>
                <input
                  className="input input--number"
                  type="number"
                  min={1}
                  value={criteria.maxEmployees ?? ''}
                  onChange={(e) => setCriteria({ ...criteria, maxEmployees: e.target.value ? Number(e.target.value) : null })}
                />
              </label>
              <label className="field">
                <span className="field__label">Number of companies</span>
                <input
                  className="input input--number"
                  type="number"
                  min={1}
                  max={50}
                  placeholder="20"
                  value={criteria.limit ?? ''}
                  onChange={(e) => setCriteria({ ...criteria, limit: e.target.value ? Number(e.target.value) : null })}
                />
              </label>
            </div>
            <div className="biz-card__actions">
              <button type="button" className="button button--ghost button--small" onClick={() => setCriteria(EMPTY_CRITERIA)}>
                Clear criteria
              </button>
            </div>
          </div>
        )}
      </form>

      {!results && !run.running && reviewed && (
        <div className="biz-examples" aria-label="Examples">
          {EXAMPLES.map((example) => (
            <button
              key={example}
              type="button"
              className="biz-example"
              onClick={() => {
                setQuery(example);
                search(example);
              }}
            >
              {example}
            </button>
          ))}
        </div>
      )}
      {run.error && (
        <p className="form-error" role="alert">
          {run.error}
        </p>
      )}
      {results && (
        <ClientResultsView
          results={results}
          useCases={(offer?.reviewed?.useCases ?? []).map((c) => c.text).filter(Boolean)}
          onSaved={markSaved}
          onOpenOpportunity={onOpenOpportunity}
        />
      )}
    </div>
  );
}

function ClientResultsView({
  results,
  useCases,
  onSaved,
  onOpenOpportunity,
}: {
  results: ClientResults;
  useCases: string[];
  onSaved: (companyKey: string, opportunityId: string) => void;
  onOpenOpportunity: (id: string) => void;
}) {
  const total = results.confirmed.length + results.needsVerification.length + results.excluded.length;
  return (
    <section className="biz-results" aria-label="Client results">
      <div className="biz-results__summary">
        <span className="biz-results__offer">
          Offer: {results.offer.name} v{results.offer.version}
        </span>
        <StatusBadge status={results.status} />
        <span className="biz-muted">Retrieved {formatDateTime(results.retrievedAt)}</span>
      </div>
      <Chips items={describeCriteria(results.criteria)} label="Criteria used" />
      {results.icp.length > 0 && (
        <div className="biz-icp">
          <span className="biz-icp__title">Ideal customer profile — a hypothesis, not confirmed demand</span>
          <Chips items={results.icp} label="ICP hypothesis" />
        </div>
      )}
      {total === 0 && results.status !== 'failed' && results.status !== 'offline' && (
        <p className="biz-empty">
          No company in the searched sources met the criteria. That is not proof there is no market — try other places
          or industries.
        </p>
      )}
      <ProspectGroup
        title="Confirmed matches"
        hint="Every hard requirement passed."
        prospects={results.confirmed}
        runId={results.runId}
        useCases={useCases}
        onSaved={onSaved}
        onOpenOpportunity={onOpenOpportunity}
        offerLabel={`${results.offer.name} v${results.offer.version}`}
        defaultOpen
      />
      <ProspectGroup
        title="Needs verification"
        hint="A required fact is unknown; nothing was assumed."
        prospects={results.needsVerification}
        runId={results.runId}
        useCases={useCases}
        onSaved={onSaved}
        onOpenOpportunity={onOpenOpportunity}
        offerLabel={`${results.offer.name} v${results.offer.version}`}
        defaultOpen
      />
      <ProspectGroup
        title="Excluded"
        hint="A hard requirement failed; kept separately rather than hidden."
        prospects={results.excluded}
        runId={results.runId}
        useCases={useCases}
        onSaved={onSaved}
        onOpenOpportunity={onOpenOpportunity}
        offerLabel={`${results.offer.name} v${results.offer.version}`}
      />
      <SearchedDetails notes={results.notes} sources={results.sources} failures={results.failures} />
      <p className="biz-footnote">
        Fit uses the scoring policy {results.scoringPolicy}: a transparent evidence measure, not a chance of buying.
      </p>
    </section>
  );
}

function ProspectGroup({
  title,
  hint,
  prospects,
  runId,
  useCases,
  offerLabel,
  onSaved,
  onOpenOpportunity,
  defaultOpen = false,
}: {
  title: string;
  hint: string;
  prospects: ClientProspect[];
  runId: string;
  useCases: string[];
  offerLabel: string;
  onSaved: (companyKey: string, opportunityId: string) => void;
  onOpenOpportunity: (id: string) => void;
  defaultOpen?: boolean;
}) {
  const [open, setOpen] = useState(defaultOpen);
  const [expanded, setExpanded] = useState<string | null>(null);
  if (prospects.length === 0) return null;
  return (
    <div className="biz-group">
      <button type="button" className="biz-group__head" aria-expanded={open} onClick={() => setOpen(!open)}>
        {open ? <ChevronDownIcon className="biz-group__chevron" /> : <ChevronRightIcon className="biz-group__chevron" />}
        <span className="biz-group__title">{title}</span>
        <span className="biz-group__count">{prospects.length}</span>
        <span className="biz-muted biz-group__hint">{hint}</span>
      </button>
      {open && (
        <div className="table-wrap">
          <table className="data-table biz-table">
            <thead>
              <tr>
                <th scope="col">Company</th>
                <th scope="col">Location</th>
                <th scope="col">Why it fits</th>
                <th scope="col">Observed signal</th>
                <th scope="col">Relevant buyer / contact</th>
                <th scope="col">Fit / evidence</th>
                <th scope="col">Links</th>
              </tr>
            </thead>
            <tbody>
              {prospects.map((p) => {
                const isOpen = expanded === p.companyKey;
                const contact = p.contacts[0];
                return (
                  <Fragment key={p.companyKey}>
                    <tr className={isOpen ? 'biz-table__row is-open' : 'biz-table__row'} onClick={() => setExpanded(isOpen ? null : p.companyKey)}>
                      <td className="biz-table__name">
                        <button
                          type="button"
                          className="link-button"
                          aria-expanded={isOpen}
                          onClick={(e) => {
                            e.stopPropagation();
                            setExpanded(isOpen ? null : p.companyKey);
                          }}
                        >
                          {p.companyName}
                        </button>
                        {p.suppressed && <span className="badge badge--danger">Do not contact</span>}
                        {p.opportunityId && <span className="badge badge--brand">In Pipeline</span>}
                      </td>
                      <td>{p.locations.join(' / ') || <span className="biz-muted">Unknown</span>}</td>
                      <td className="biz-table__why">
                        {p.whyItFits.length === 0 ? (
                          <span className="biz-muted">—</span>
                        ) : (
                          <>
                            {p.whyItFits.slice(0, 2).join('; ')}
                            {p.whyItFits.length > 2 && (
                              <span className="biz-muted biz-person__title">+{p.whyItFits.length - 2} more in details</span>
                            )}
                          </>
                        )}
                      </td>
                      <td className="biz-table__signal">
                        {p.observedSignals[0] ?? <span className="biz-muted">None found</span>}
                      </td>
                      <td>
                        {contact ? (
                          <>
                            <span className="biz-person">{contact.name ?? contact.role}</span>
                            {contact.name && <span className="biz-muted biz-person__title">{contact.title ?? contact.role}</span>}
                            <span className="biz-muted biz-person__title">{AUTHORITY_LABELS[contact.authority]}</span>
                          </>
                        ) : (
                          <span className="biz-muted">Role not identified</span>
                        )}
                      </td>
                      <td className="biz-table__fit">
                        <FitCell score={p.assessment.score} coverage={p.assessment.coverage} shown={p.assessment.scoreShown} />
                      </td>
                      <td>
                        <Links links={prospectLinks(p)} />
                      </td>
                    </tr>
                    {isOpen && (
                      <tr className="biz-table__detail">
                        <td colSpan={7}>
                          <ProspectDetail
                            prospect={p}
                            runId={runId}
                            useCases={useCases}
                            offerLabel={offerLabel}
                            onSaved={onSaved}
                            onOpenOpportunity={onOpenOpportunity}
                          />
                        </td>
                      </tr>
                    )}
                  </Fragment>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}

function prospectLinks(p: ClientProspect) {
  const links: { label: string; url: string }[] = [];
  if (p.website) links.push({ label: 'Website', url: p.website });
  const contactPage = p.contacts.find((c) => c.contactPage)?.contactPage;
  if (contactPage && contactPage !== p.website) links.push({ label: 'Contact page', url: contactPage });
  for (const url of p.links) {
    if (!links.some((l) => l.url === url) && links.length < 4) links.push({ label: 'Source', url });
  }
  return links;
}

/** The expandable row (B11): version, evidence both ways, unknowns, saving. */
function ProspectDetail({
  prospect: p,
  runId,
  useCases,
  offerLabel,
  onSaved,
  onOpenOpportunity,
}: {
  prospect: ClientProspect;
  runId: string;
  useCases: string[];
  offerLabel: string;
  onSaved: (companyKey: string, opportunityId: string) => void;
  onOpenOpportunity: (id: string) => void;
}) {
  // The offer's own use case the evidence mentions, else its first one.
  const mentioned = useCases.find((u) => p.whyItFits.some((w) => w.toLowerCase().includes(u.toLowerCase())));
  const [useCase, setUseCase] = useState(mentioned ?? useCases[0] ?? '');
  const listId = `biz-usecases-${p.companyKey}`;
  const [saved, setSaved] = useState<string | null>(null);
  const action = useAction();
  const a = p.assessment;
  const supporting = a.evidence.filter((e) => !e.contrary);
  const contrary = a.evidence.filter((e) => e.contrary);
  return (
    <div className="biz-detail">
      <div className="biz-detail__cols">
        <div>
          <h4 className="biz-detail__title">Fit against {offerLabel}</h4>
          <p className="biz-detail__score">{fitLabel(a.score, a.coverage ?? 0, a.scoreShown)}</p>
          <ul className="biz-checks">
            {a.hard.map((h) => (
              <li key={h.name} className={`biz-checks__item is-${h.result}`}>
                <span className="biz-checks__result">{h.result === 'pass' ? 'Pass' : h.result === 'fail' ? 'Fail' : 'Unknown'}</span>
                <span>
                  <strong>{h.name}</strong> — {h.detail}
                </span>
              </li>
            ))}
          </ul>
          <table className="biz-criteria-table">
            <thead>
              <tr>
                <th scope="col">Criterion</th>
                <th scope="col">Weight</th>
                <th scope="col">Match</th>
              </tr>
            </thead>
            <tbody>
              {a.criteria.map((c) => (
                <tr key={c.id}>
                  <td>
                    {c.label}
                    <span className="biz-muted biz-criteria-table__reason">{c.reason}</span>
                  </td>
                  <td className="biz-num">{c.applicable && c.weight !== null ? `${Math.round(c.weight * 100)}%` : 'n/a'}</td>
                  <td className="biz-num">
                    {!c.applicable ? 'Not applicable' : c.value === null ? 'Unknown' : `${Math.round(c.value * 100)}%`}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          <p className="biz-footnote">
            Assessed {formatDateTime(a.createdAt)} · policy {a.policyVersion}
          </p>
        </div>
        <div>
          <h4 className="biz-detail__title">Signals and intent</h4>
          <dl className="biz-facts">
            <dt>Observed signals</dt>
            <dd>{p.observedSignals.length ? p.observedSignals.join('; ') : 'None found in the inspected sources.'}</dd>
            <dt>Verified buying intent</dt>
            <dd>{p.verifiedBuyingIntent}</dd>
            <dt>Permission to contact</dt>
            <dd>{p.permissionToContact}</dd>
            {p.industry && (
              <>
                <dt>Industry</dt>
                <dd>{p.industry}</dd>
              </>
            )}
            {p.size && (
              <>
                <dt>Size</dt>
                <dd>{p.size}</dd>
              </>
            )}
          </dl>
          {p.missing.length > 0 && (
            <p className="biz-missing">
              <strong>Missing:</strong> {p.missing.join('; ')}
            </p>
          )}
          {p.contacts.length > 0 && (
            <>
              <h4 className="biz-detail__title">Buyer roles and contacts</h4>
              <ul className="biz-contacts">
                {p.contacts.map((c, i) => (
                  <li key={i}>
                    <span className="biz-person">{c.name ?? c.role}</span>
                    {c.name && <span className="biz-muted"> · {c.title ?? c.role}</span>}
                    <span className="badge">{AUTHORITY_LABELS[c.authority]}</span>
                    {c.suppressed && <span className="badge badge--danger">Do not contact</span>}
                    <span className="biz-contacts__reason">{c.reason}</span>
                    <Links
                      links={[
                        c.profileUrl ? { label: 'Profile', url: c.profileUrl } : null,
                        c.contactPage ? { label: 'Contact page', url: c.contactPage } : null,
                        c.sourceUrl ? { label: 'Source', url: c.sourceUrl } : null,
                      ].filter((l): l is { label: string; url: string } => l !== null)}
                    />
                  </li>
                ))}
              </ul>
            </>
          )}
        </div>
      </div>
      <h4 className="biz-detail__title">Supporting evidence</h4>
      <SourceNotes notes={supporting} empty="No supporting evidence retained." />
      {contrary.length > 0 && (
        <>
          <h4 className="biz-detail__title">Contrary evidence</h4>
          <SourceNotes notes={contrary} />
        </>
      )}
      <p className="biz-approach">
        <strong>Suggested approach:</strong> ask {p.contacts[0]?.role ?? 'the relevant team'} how they handle{' '}
        {(useCase || 'this use case').toLowerCase()} today — the need is a hypothesis until they confirm it.
      </p>
      <div className="biz-save">
        {p.opportunityId ? (
          <button type="button" className="button button--secondary button--small" onClick={() => onOpenOpportunity(p.opportunityId ?? '')}>
            Open in Pipeline
          </button>
        ) : (
          <>
            <label className="field biz-save__field">
              <span className="field__label">Use case for this opportunity</span>
              <input className="input" list={listId} value={useCase} onChange={(e) => setUseCase(e.target.value)} />
              <datalist id={listId}>
                {useCases.map((u) => (
                  <option key={u} value={u} />
                ))}
              </datalist>
            </label>
            <button
              type="button"
              className="button button--primary button--small"
              disabled={action.busy}
              onClick={() =>
                void action.run(async () => {
                  const result = await saveProspect(runId, p.companyKey, useCase.trim() || null);
                  onSaved(p.companyKey, result.opportunity.id);
                  setSaved(result.created ? 'Saved as a New Lead.' : 'Already in your Pipeline: the new evidence was added.');
                })
              }
            >
              Save to Pipeline
            </button>
          </>
        )}
        {saved && (
          <span className="biz-saved" role="status">
            {saved}
          </span>
        )}
        {action.error && (
          <span className="form-error" role="alert">
            {action.error}
          </span>
        )}
      </div>
    </div>
  );
}
