import { useMemo, useState } from 'react';

import { useAction } from '../../hooks/useAction';
import { dataOr } from '../../hooks/useAsyncData';
import { usePipeline } from '../../hooks/useBusiness';
import { formatDate, formatDateTime } from '../../lib/format';
import {
  createOpportunity,
  liftSuppression,
  requestKey,
  type BusinessOverview,
  type Opportunity,
  type OpportunityKind,
  type PipelineStage,
  type Suppression,
} from '../../services/businessService';
import { PlusIcon } from '../icons';
import { Dialog } from '../ui/Dialog';
import { EmptyState, LoadingState } from '../ui/EmptyState';
import { OPPORTUNITY_KIND_LABELS, STAGE_LABELS, STAGES } from './labels';
import { OpportunityDetail } from './OpportunityDetail';

function lastActivity(o: Opportunity): string {
  const latest = o.activities
    .filter((a) => a.kind !== 'stage_change')
    .sort((a, b) => b.occurredAt - a.occurredAt)[0];
  if (!latest) return '—';
  return formatDate(latest.occurredAt);
}

function amount(o: Opportunity): string {
  if (o.amount) return `${o.amount.currency ? `${o.amount.currency} ` : ''}${o.amount.value} · ${o.amount.basis}`;
  if (o.contract && (o.contract.rateMin !== null || o.contract.rateMax !== null)) {
    const t = o.contract;
    const value = t.rateMin !== null && t.rateMax !== null && t.rateMin !== t.rateMax ? `${t.rateMin}–${t.rateMax}` : `${t.rateMin ?? t.rateMax}`;
    return `${t.currency ?? ''} ${value}${t.rateUnit ? `/${t.rateUnit}` : ''} · advertised`.trim();
  }
  return '—';
}

/**
 * The Business Pipeline (B15): one commercial source of truth, separate
 * from job Applications. Stages move only on what the user records.
 */
export function Pipeline({
  overview,
  focusId,
  onFocus,
}: {
  overview: BusinessOverview;
  focusId: string | null;
  onFocus: (id: string | null) => void;
}) {
  const pipeline = usePipeline();
  const data = dataOr(pipeline.state, null);
  const [stage, setStage] = useState<PipelineStage | 'all'>('all');
  const [kind, setKind] = useState<OpportunityKind | 'all'>('all');
  const [showArchived, setShowArchived] = useState(false);
  const [text, setText] = useState('');
  const [adding, setAdding] = useState(false);

  const visible = useMemo(() => {
    const needle = text.trim().toLowerCase();
    return (data?.opportunities ?? []).filter(
      (o) =>
        (showArchived || !o.archived) &&
        (stage === 'all' || o.stage === stage) &&
        (kind === 'all' || o.kind === kind) &&
        (!needle ||
          [o.name, o.companyName ?? '', o.useCase, o.nextStep, o.offer?.name ?? ''].some((v) => v.toLowerCase().includes(needle))),
    );
  }, [data, stage, kind, showArchived, text]);

  const focused = data?.opportunities.find((o) => o.id === focusId) ?? null;
  const counts = new Map(data?.counts ?? []);

  if (pipeline.state.status === 'loading') return <LoadingState label="Loading your Pipeline…" />;
  if (pipeline.state.status === 'error') {
    return (
      <div className="notice notice--danger" role="alert">
        {pipeline.state.error.message}{' '}
        <button type="button" className="link-button" onClick={pipeline.retry}>
          Try again
        </button>
      </div>
    );
  }

  return (
    <div className="biz-view">
      <div className="biz-stages" role="group" aria-label="Filter by stage">
        <button
          type="button"
          className={stage === 'all' ? 'biz-stage is-active' : 'biz-stage'}
          aria-pressed={stage === 'all'}
          onClick={() => setStage('all')}
        >
          <span className="biz-stage__label">All</span>
          <span className="biz-stage__count">{data?.opportunities.filter((o) => showArchived || !o.archived).length ?? 0}</span>
        </button>
        {STAGES.map((s) => (
          <button
            key={s}
            type="button"
            className={stage === s ? `biz-stage biz-stage--${s} is-active` : `biz-stage biz-stage--${s}`}
            aria-pressed={stage === s}
            onClick={() => setStage(stage === s ? 'all' : s)}
          >
            <span className="biz-stage__label">{STAGE_LABELS[s]}</span>
            <span className="biz-stage__count">{counts.get(s) ?? 0}</span>
          </button>
        ))}
      </div>

      <div className="biz-toolbar">
        <input
          className="input biz-toolbar__search"
          type="search"
          aria-label="Search the pipeline"
          placeholder="Search…"
          value={text}
          onChange={(e) => setText(e.target.value)}
        />
        <select className="input biz-toolbar__select" aria-label="Type" value={kind} onChange={(e) => setKind(e.target.value as OpportunityKind | 'all')}>
          <option value="all">All types</option>
          {(Object.keys(OPPORTUNITY_KIND_LABELS) as OpportunityKind[]).map((k) => (
            <option key={k} value={k}>
              {OPPORTUNITY_KIND_LABELS[k]}
            </option>
          ))}
        </select>
        <label className="biz-check">
          <input type="checkbox" checked={showArchived} onChange={(e) => setShowArchived(e.target.checked)} />
          Show archived
        </label>
        <span className="biz-search__spacer" />
        <button type="button" className="button button--secondary button--small" onClick={() => setAdding(true)}>
          <PlusIcon className="button__icon" />
          Add opportunity
        </button>
      </div>

      {data && data.opportunities.length === 0 ? (
        <EmptyState title="Nothing saved yet" framed>
          Save a client from Find Clients or a listing from Find Contract Work, or add an opportunity you already know.
          Saved opportunities start as New Lead.
        </EmptyState>
      ) : visible.length === 0 ? (
        <p className="biz-empty">No opportunity matches these filters.</p>
      ) : (
        <div className="table-wrap">
          <table className="data-table biz-table">
            <thead>
              <tr>
                <th scope="col">Company / client</th>
                <th scope="col">Opportunity / offer</th>
                <th scope="col">Type</th>
                <th scope="col">Stage</th>
                <th scope="col">Relevant contact</th>
                <th scope="col">Last activity</th>
                <th scope="col">Next step</th>
                <th scope="col">Amount / basis</th>
              </tr>
            </thead>
            <tbody>
              {visible.map((o) => (
                <tr key={o.id} className="biz-table__row" onClick={() => onFocus(o.id)}>
                  <td className="biz-table__name">
                    <button
                      type="button"
                      className="link-button"
                      onClick={(e) => {
                        e.stopPropagation();
                        onFocus(o.id);
                      }}
                    >
                      {o.companyName ?? (o.kind === 'contract' ? 'Client not disclosed' : o.name)}
                    </button>
                    {o.doNotContact && <span className="badge badge--danger">Do not contact</span>}
                    {o.archived && <span className="badge">Archived</span>}
                  </td>
                  <td>
                    {o.name}
                    {o.offer && <span className="biz-muted biz-person__title">{`${o.offer.name} v${o.offer.version}`}</span>}
                  </td>
                  <td>{OPPORTUNITY_KIND_LABELS[o.kind]}</td>
                  <td>
                    <span className={`biz-stage-tag biz-stage-tag--${o.stage}`}>{STAGE_LABELS[o.stage]}</span>
                  </td>
                  <td>
                    {o.contacts[0] ? (
                      <>
                        {o.contacts[0].name ?? o.contacts[0].role}
                        {o.contacts[0].name && <span className="biz-muted biz-person__title">{o.contacts[0].title ?? o.contacts[0].role}</span>}
                      </>
                    ) : (
                      <span className="biz-muted">—</span>
                    )}
                  </td>
                  <td>{lastActivity(o)}</td>
                  <td>{o.nextStep || <span className="biz-muted">—</span>}</td>
                  <td>{amount(o)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      {data && data.suppressions.length > 0 && <Suppressions items={data.suppressions} />}

      {focused && (
        <OpportunityDetail
          opportunity={focused}
          overview={overview}
          suppressions={data?.suppressions ?? []}
          onClose={() => onFocus(null)}
        />
      )}
      {adding && <NewOpportunityDialog overview={overview} onClose={() => setAdding(false)} onCreated={(id) => onFocus(id)} />}
    </div>
  );
}

/** Do-not-contact entries; lifting one is only ever the user's own action (B29). */
function Suppressions({ items }: { items: Suppression[] }) {
  const [open, setOpen] = useState(false);
  const [confirm, setConfirm] = useState<string | null>(null);
  const action = useAction();
  return (
    <section className="biz-card biz-suppressions" aria-label="Do not contact">
      <button type="button" className="biz-group__head" aria-expanded={open} onClick={() => setOpen(!open)}>
        <span className="biz-group__title">Do not contact</span>
        <span className="biz-group__count">{items.length}</span>
        <span className="biz-muted biz-group__hint">Research and drafting respect these; ReMa never lifts them.</span>
      </button>
      {open && (
        <ul className="biz-suppressions__list">
          {items.map((s) => (
            <li key={s.id}>
              <span className="biz-person">{s.label}</span>
              <span className="biz-muted">
                {s.scope === 'person' ? 'Person' : 'Company'} · since {formatDateTime(s.createdAt)}
                {s.reason && ` · ${s.reason}`}
              </span>
              {confirm === s.id ? (
                <span className="biz-confirm-inline">
                  <span>Lift it? You decide this, not ReMa.</span>
                  <button
                    type="button"
                    className="button button--secondary button--small"
                    onClick={() => void action.run(() => liftSuppression(s.id)).then(() => setConfirm(null))}
                  >
                    Lift
                  </button>
                  <button type="button" className="button button--ghost button--small" onClick={() => setConfirm(null)}>
                    Keep
                  </button>
                </span>
              ) : (
                <button type="button" className="link-button" onClick={() => setConfirm(s.id)}>
                  Lift…
                </button>
              )}
            </li>
          ))}
        </ul>
      )}
      {action.error && (
        <p className="form-error" role="alert">
          {action.error}
        </p>
      )}
    </section>
  );
}

function NewOpportunityDialog({
  overview,
  onClose,
  onCreated,
}: {
  overview: BusinessOverview;
  onClose: () => void;
  onCreated: (id: string) => void;
}) {
  const reviewed = overview.offers.filter((o) => o.currentVersion !== null && !o.archived);
  const [kind, setKind] = useState<OpportunityKind>('service');
  const [name, setName] = useState('');
  const [company, setCompany] = useState('');
  const [offerId, setOfferId] = useState(reviewed[0]?.id ?? '');
  const [useCase, setUseCase] = useState('');
  const [sourceUrl, setSourceUrl] = useState('');
  const [key] = useState(() => requestKey('opp'));
  const action = useAction();
  const submit = () =>
    void action.run(async () => {
      const created = await createOpportunity({
        kind,
        name: name.trim(),
        companyName: company.trim() || null,
        offerId: kind === 'contract' ? null : offerId || null,
        useCase: useCase.trim(),
        sourceUrl: sourceUrl.trim() || null,
        amount: null,
        idempotencyKey: key,
      });
      onClose();
      onCreated(created.id);
    });
  return (
    <Dialog
      title="Add opportunity"
      onClose={onClose}
      actions={
        <>
          <button type="button" className="button button--ghost" onClick={onClose}>
            Cancel
          </button>
          <button type="button" className="button button--primary" disabled={!name.trim() || action.busy} onClick={submit}>
            Add as New Lead
          </button>
        </>
      }
    >
      <div className="biz-dialog-form">
        <label className="field">
          <span className="field__label">Type</span>
          <select className="input" value={kind} onChange={(e) => setKind(e.target.value as OpportunityKind)}>
            {(Object.keys(OPPORTUNITY_KIND_LABELS) as OpportunityKind[]).map((k) => (
              <option key={k} value={k}>
                {OPPORTUNITY_KIND_LABELS[k]}
              </option>
            ))}
          </select>
        </label>
        <label className="field">
          <span className="field__label">Name</span>
          <input className="input" value={name} placeholder="e.g. Support automation for Acme" onChange={(e) => setName(e.target.value)} />
        </label>
        <label className="field">
          <span className="field__label">Company / client (optional)</span>
          <input className="input" value={company} onChange={(e) => setCompany(e.target.value)} />
        </label>
        {kind !== 'contract' && reviewed.length > 0 && (
          <label className="field">
            <span className="field__label">Offer</span>
            <select className="input" value={offerId} onChange={(e) => setOfferId(e.target.value)}>
              <option value="">None</option>
              {reviewed.map((o) => (
                <option key={o.id} value={o.id}>
                  {o.name} v{o.currentVersion}
                </option>
              ))}
            </select>
          </label>
        )}
        <label className="field">
          <span className="field__label">Use case (optional)</span>
          <input className="input" value={useCase} onChange={(e) => setUseCase(e.target.value)} />
        </label>
        <label className="field">
          <span className="field__label">Source link (optional)</span>
          <input className="input" type="url" value={sourceUrl} onChange={(e) => setSourceUrl(e.target.value)} />
        </label>
        {action.error && (
          <p className="form-error" role="alert">
            {action.error}
          </p>
        )}
      </div>
    </Dialog>
  );
}
