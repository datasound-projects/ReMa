import { useState } from 'react';

import { useAction } from '../../hooks/useAction';
import { useBusinessRun } from '../../hooks/useBusiness';
import { formatDate, formatDateTime } from '../../lib/format';
import {
  changeStage,
  deleteActivity,
  deleteContact,
  deleteOpportunity,
  doNotContact,
  editOpportunity,
  reassessOpportunity,
  recordActivity,
  refreshListing,
  requestKey,
  suppressContact,
  type ActivityType,
  type Amount,
  type BusinessOverview,
  type Opportunity,
  type PipelineStage,
} from '../../services/businessService';
import { TrashIcon } from '../icons';
import { Dialog } from '../ui/Dialog';
import { Links, RunProgress, SourceNotes } from './common';
import { occurredAt, toDateInput } from './helpers';
import { DraftCard, NewDraftForm } from './Drafts';
import { TermsFacts } from './FindContracts';
import {
  ACTIVITY_LABELS,
  fitLabel,
  OPPORTUNITY_KIND_LABELS,
  RECORDABLE,
  STAGE_LABELS,
  STAGE_MEANINGS,
  STAGES,
} from './labels';

const RANK: Record<PipelineStage, number> = {
  new_lead: 0,
  qualified: 1,
  contacted: 2,
  discussion: 3,
  proposal: 4,
  won: 5,
  lost: 5,
};

/** What each stage rests on (the backend enforces the same, B15). */
const REQUIRED: Record<PipelineStage, ActivityType[]> = {
  new_lead: [],
  qualified: [],
  contacted: ['contact'],
  discussion: ['reply', 'positive_reply', 'meeting_held'],
  proposal: ['proposal_sent'],
  won: ['won'],
  lost: ['lost'],
};

type Section = 'overview' | 'activity' | 'contacts' | 'drafts';

/** One opportunity: edit, stage, actual activity, contacts, drafts (B15). */
export function OpportunityDetail({
  opportunity: o,
  overview,
  onClose,
}: {
  opportunity: Opportunity;
  overview: BusinessOverview;
  onClose: () => void;
}) {
  const [section, setSection] = useState<Section>('overview');
  const drafts = overview.drafts.filter((d) => d.opportunityId === o.id);
  const experiments = overview.experiments.filter(
    (e) => e.status !== 'cancelled' && e.content.cohort.some((c) => c.opportunityId === o.id || c.accountKey === o.companyKey),
  );
  return (
    <Dialog
      title={o.name}
      size="wide"
      onClose={onClose}
      actions={
        <button type="button" className="button button--primary" onClick={onClose}>
          Close
        </button>
      }
    >
      <div className="biz-opp">
        <div className="biz-opp__badges">
          <span className="badge badge--brand">{STAGE_LABELS[o.stage]}</span>
          <span className="badge">{OPPORTUNITY_KIND_LABELS[o.kind]}</span>
          {o.archived && <span className="badge">Archived</span>}
          {o.doNotContact && <span className="badge badge--danger">Do not contact</span>}
          {o.listingStatus && <span className="badge badge--warning">{o.listingStatus}</span>}
        </div>
        <div className="profile-tabs biz-opp__tabs" role="tablist" aria-label="Opportunity">
          {(
            [
              ['overview', 'Overview'],
              ['activity', `Stage & activity (${o.activities.length})`],
              ['contacts', `Contacts (${o.contacts.length})`],
              ['drafts', `Drafts (${drafts.length})`],
            ] as [Section, string][]
          ).map(([id, label]) => (
            <button
              key={id}
              type="button"
              role="tab"
              aria-selected={section === id}
              className={section === id ? 'profile-tabs__tab profile-tabs__tab--active' : 'profile-tabs__tab'}
              onClick={() => setSection(id)}
            >
              {label}
            </button>
          ))}
        </div>
        {section === 'overview' && <Overview key={o.revision} o={o} overview={overview} onDeleted={onClose} />}
        {section === 'activity' && (
          <>
            <StageForm key={`${o.stage}-${o.revision}`} o={o} />
            <ActivityForm o={o} experiments={experiments.map((e) => ({ id: e.id, label: e.content.hypothesis }))} />
            <History o={o} />
          </>
        )}
        {section === 'contacts' && <Contacts o={o} />}
        {section === 'drafts' && (
          <div className="biz-opp__drafts">
            {o.doNotContact ? (
              <p className="notice notice--danger">This opportunity is marked Do not contact: ReMa does not draft messages for it.</p>
            ) : o.offer === null ? (
              <p className="biz-muted">Drafts need a reviewed offer version; this opportunity has none.</p>
            ) : (
              <NewDraftForm
                opportunityId={o.id}
                planId={null}
                contacts={o.contacts}
                defaultRole={o.contacts[0]?.role ?? ''}
                experiments={experiments}
              />
            )}
            {drafts.map((d) => (
              <DraftCard key={`${d.id}-${d.revision}`} draft={d} />
            ))}
          </div>
        )}
      </div>
    </Dialog>
  );
}

function amountText(a: Amount | null) {
  if (!a) return 'Unknown';
  return `${a.currency ? `${a.currency} ` : ''}${a.value} (${a.basis}; ${a.source})`;
}

function Overview({ o, overview, onDeleted }: { o: Opportunity; overview: BusinessOverview; onDeleted: () => void }) {
  const [name, setName] = useState(o.name);
  const [useCase, setUseCase] = useState(o.useCase);
  const [nextStep, setNextStep] = useState(o.nextStep);
  const [notes, setNotes] = useState(o.notes);
  const [archived, setArchived] = useState(o.archived);
  const [amountValue, setAmountValue] = useState(o.amount?.value ?? '');
  const [amountCurrency, setAmountCurrency] = useState(o.amount?.currency ?? 'EUR');
  const [amountBasis, setAmountBasis] = useState(o.amount?.basis ?? 'estimate');
  const [confirm, setConfirm] = useState<'dnc' | 'delete' | null>(null);
  const [reason, setReason] = useState('');
  const [saved, setSaved] = useState(false);
  const action = useAction();
  const reassess = useBusinessRun();
  const offer = o.offer ? overview.offers.find((x) => x.id === o.offer?.offerId) ?? null : null;
  const [version, setVersion] = useState<number | null>(offer?.currentVersion ?? null);
  const listing = useAction();

  const save = () =>
    void action
      .run(() =>
        editOpportunity(
          o.id,
          {
            name,
            useCase,
            nextStep,
            notes,
            archived,
            amount: amountValue.trim()
              ? { value: amountValue.trim(), currency: amountCurrency.trim() || null, basis: amountBasis, source: 'user_stated' }
              : null,
          },
          o.revision,
        ),
      )
      .then((ok) => setSaved(ok));

  return (
    <div className="biz-opp__overview">
      <dl className="biz-facts">
        <dt>Company / client</dt>
        <dd>{o.companyName ?? (o.kind === 'contract' ? 'End client not disclosed' : 'Not recorded')}</dd>
        <dt>Offer</dt>
        <dd>{o.offer ? `${o.offer.name} v${o.offer.version}` : '—'}</dd>
        <dt>Saved</dt>
        <dd>{formatDateTime(o.createdAt)}</dd>
        <dt>Last researched</dt>
        <dd>{o.lastResearchedAt !== null ? formatDateTime(o.lastResearchedAt) : '—'}</dd>
        <dt>Last commercial activity</dt>
        <dd>{o.lastCommercialActivityAt !== null ? formatDateTime(o.lastCommercialActivityAt) : 'None recorded'}</dd>
        <dt>Amount</dt>
        <dd>{amountText(o.amount)}</dd>
        {o.sourceUrl && (
          <>
            <dt>Source</dt>
            <dd>
              <Links links={[{ label: 'Open source', url: o.sourceUrl }]} />
            </dd>
          </>
        )}
      </dl>

      {o.assessment && (
        <section className="biz-opp__block" aria-label="Fit assessment">
          <h4 className="biz-detail__title">Fit assessment</h4>
          <p>
            {fitLabel(o.assessment.score, o.assessment.coverage ?? 0, o.assessment.scoreShown)} against{' '}
            {o.assessment.offer.name} v{o.assessment.offer.version} · {formatDate(o.assessment.createdAt)} ·{' '}
            {o.assessments} assessment{o.assessments === 1 ? '' : 's'} kept
          </p>
          {offer && offer.currentVersion !== null && (
            <div className="biz-inline-form">
              <label className="field">
                <span className="field__label">Reassess against version</span>
                <select className="input" value={version ?? ''} onChange={(e) => setVersion(Number(e.target.value))}>
                  {Array.from({ length: offer.currentVersion }, (_, i) => i + 1)
                    .reverse()
                    .map((v) => (
                      <option key={v} value={v}>
                        v{v}
                      </option>
                    ))}
                </select>
              </label>
              {reassess.running ? (
                <RunProgress status={reassess.status} fallback="Reassessing…" onStop={reassess.stop} />
              ) : (
                <button
                  type="button"
                  className="button button--secondary button--small"
                  onClick={() => void reassess.start((runId) => reassessOpportunity(o.id, runId, version))}
                >
                  Reassess
                </button>
              )}
            </div>
          )}
          {reassess.error && (
            <p className="form-error" role="alert">
              {reassess.error}
            </p>
          )}
        </section>
      )}

      {o.contract && (
        <section className="biz-opp__block" aria-label="Contract terms">
          <h4 className="biz-detail__title">Contract terms as advertised</h4>
          <TermsFacts terms={o.contract} />
          <div className="biz-inline-form">
            <button
              type="button"
              className="button button--secondary button--small"
              disabled={listing.busy}
              onClick={() => void listing.run(() => refreshListing(o.id))}
            >
              {listing.busy ? 'Checking…' : 'Check listing'}
            </button>
            <span className="biz-muted">A closed listing keeps your notes and stage.</span>
          </div>
          {listing.error && (
            <p className="form-error" role="alert">
              {listing.error}
            </p>
          )}
        </section>
      )}

      <section className="biz-opp__block" aria-label="Edit">
        <div className="biz-form-grid">
          <label className="field">
            <span className="field__label">Name</span>
            <input className="input" value={name} onChange={(e) => setName(e.target.value)} />
          </label>
          <label className="field">
            <span className="field__label">Use case</span>
            <input className="input" value={useCase} onChange={(e) => setUseCase(e.target.value)} />
          </label>
        </div>
        <label className="field">
          <span className="field__label">Next step</span>
          <input className="input" value={nextStep} onChange={(e) => setNextStep(e.target.value)} />
        </label>
        <label className="field">
          <span className="field__label">Notes</span>
          <textarea className="input input--textarea" rows={3} value={notes} onChange={(e) => setNotes(e.target.value)} />
        </label>
        <div className="biz-form-grid biz-form-grid--rate">
          <label className="field">
            <span className="field__label">Amount (value or range)</span>
            <input className="input" value={amountValue} onChange={(e) => setAmountValue(e.target.value)} />
          </label>
          <label className="field">
            <span className="field__label">Currency</span>
            <input className="input" maxLength={3} value={amountCurrency} onChange={(e) => setAmountCurrency(e.target.value.toUpperCase())} />
          </label>
          <label className="field">
            <span className="field__label">Basis</span>
            <select className="input" value={amountBasis} onChange={(e) => setAmountBasis(e.target.value)}>
              <option value="estimate">Estimate</option>
              <option value="advertised">Advertised</option>
              <option value="target">Your target</option>
              <option value="negotiated">Negotiated</option>
            </select>
          </label>
        </div>
        <label className="biz-check">
          <input type="checkbox" checked={archived} onChange={(e) => setArchived(e.target.checked)} />
          Archived
        </label>
        {action.error && (
          <p className="form-error" role="alert">
            {action.error}
          </p>
        )}
        <div className="biz-card__actions">
          {saved && (
            <span className="biz-saved" role="status">
              Saved
            </span>
          )}
          <button type="button" className="button button--primary button--small" disabled={action.busy} onClick={save}>
            Save changes
          </button>
        </div>
      </section>

      <section className="biz-opp__block" aria-label="Evidence">
        <h4 className="biz-detail__title">Evidence</h4>
        <SourceNotes notes={o.evidence} empty="No evidence kept." />
      </section>

      <section className="biz-opp__block biz-opp__danger" aria-label="Do not contact and delete">
        {!o.doNotContact && (
          <button type="button" className="button button--secondary button--small" onClick={() => setConfirm('dnc')}>
            Mark Do not contact
          </button>
        )}
        <button type="button" className="button button--ghost button--small" onClick={() => setConfirm('delete')}>
          <TrashIcon className="button__icon" />
          Delete opportunity
        </button>
        {confirm === 'dnc' && (
          <div className="notice notice--warning biz-confirm" role="alertdialog" aria-label="Do not contact">
            <span>
              Research and drafting will respect this; ReMa never lifts it on its own. Only you can lift it under
              Do not contact.
            </span>
            <input className="input" placeholder="Reason (optional)" value={reason} onChange={(e) => setReason(e.target.value)} />
            <button
              type="button"
              className="button button--primary button--small"
              onClick={() => void action.run(() => doNotContact(o.id, reason.trim() || null)).then(() => setConfirm(null))}
            >
              Confirm
            </button>
            <button type="button" className="button button--ghost button--small" onClick={() => setConfirm(null)}>
              Cancel
            </button>
          </div>
        )}
        {confirm === 'delete' && (
          <div className="notice notice--danger biz-confirm" role="alertdialog" aria-label="Delete opportunity">
            <span>Delete this opportunity with its activity, contacts and drafts? This cannot be undone.</span>
            <button
              type="button"
              className="button button--danger button--small"
              onClick={() =>
                void action.run(() => deleteOpportunity(o.id)).then((ok) => {
                  if (ok) onDeleted();
                })
              }
            >
              Delete
            </button>
            <button type="button" className="button button--ghost button--small" onClick={() => setConfirm(null)}>
              Cancel
            </button>
          </div>
        )}
      </section>
    </div>
  );
}

/** Moves the stage, recording what actually happened when the stage needs it. */
function StageForm({ o }: { o: Opportunity }) {
  const targets = STAGES.filter((s) => s !== o.stage);
  const [to, setTo] = useState<PipelineStage>(targets[0] ?? 'qualified');
  const [activity, setActivity] = useState<ActivityType | ''>('');
  const [today] = useState(() => toDateInput(Date.now()));
  const [date, setDate] = useState(today);
  const [person, setPerson] = useState('');
  const [reason, setReason] = useState('');
  const [amountValue, setAmountValue] = useState('');
  const [currency, setCurrency] = useState('EUR');
  const [key] = useState(() => requestKey('stage'));
  const action = useAction();
  const backward = RANK[to] < RANK[o.stage] || RANK[o.stage] === 5;
  const needs = REQUIRED[to];
  const recorded = needs.length === 0 || o.activities.some((a) => needs.includes(a.kind));
  const blocked = o.doNotContact && (to === 'contacted' || to === 'discussion' || to === 'proposal');

  return (
    <section className="biz-opp__block" aria-label="Change stage">
      <h4 className="biz-detail__title">Change stage</h4>
      <div className="biz-form-grid">
        <label className="field">
          <span className="field__label">Move to</span>
          <select
            className="input"
            value={to}
            onChange={(e) => {
              setTo(e.target.value as PipelineStage);
              setActivity('');
            }}
          >
            {targets.map((s) => (
              <option key={s} value={s}>
                {STAGE_LABELS[s]}
              </option>
            ))}
          </select>
        </label>
        {!recorded && needs.length > 1 && (
          <label className="field">
            <span className="field__label">What happened</span>
            <select className="input" value={activity} onChange={(e) => setActivity(e.target.value as ActivityType)}>
              <option value="">Choose…</option>
              {needs.map((k) => (
                <option key={k} value={k}>
                  {ACTIVITY_LABELS[k]}
                </option>
              ))}
            </select>
          </label>
        )}
        {!recorded && (
          <>
            <label className="field">
              <span className="field__label">When</span>
              <input className="input" type="date" max={today} value={date} onChange={(e) => setDate(e.target.value)} />
            </label>
            <label className="field">
              <span className="field__label">With (optional)</span>
              <input className="input" value={person} onChange={(e) => setPerson(e.target.value)} />
            </label>
          </>
        )}
        {to === 'won' && (
          <>
            <label className="field">
              <span className="field__label">Accepted value (optional)</span>
              <input className="input" value={amountValue} onChange={(e) => setAmountValue(e.target.value)} />
            </label>
            <label className="field">
              <span className="field__label">Currency</span>
              <input className="input" maxLength={3} value={currency} onChange={(e) => setCurrency(e.target.value.toUpperCase())} />
            </label>
          </>
        )}
      </div>
      <p className="form__hint">
        {STAGE_MEANINGS[to]}
        {!recorded && ` This records ${needs.length === 1 && needs[0] ? `“${ACTIVITY_LABELS[needs[0]].toLowerCase()}”` : 'what happened'} as user-reported activity.`}
      </p>
      {(backward || to === 'lost') && (
        <label className="field">
          <span className="field__label">{backward ? 'Reason (required)' : 'Reason (optional)'}</span>
          <input className="input" value={reason} onChange={(e) => setReason(e.target.value)} />
        </label>
      )}
      {blocked && <p className="notice notice--danger">Marked Do not contact: lift the suppression yourself first.</p>}
      {action.error && (
        <p className="form-error" role="alert">
          {action.error}
        </p>
      )}
      <div className="biz-card__actions">
        <button
          type="button"
          className="button button--primary button--small"
          disabled={action.busy || blocked || (backward && !reason.trim()) || (!recorded && needs.length > 1 && !activity)}
          onClick={() =>
            void action.run(() =>
              changeStage(o.id, {
                to,
                reason: reason.trim() || null,
                activity: activity || null,
                occurredAt: recorded ? null : occurredAt(date),
                person: person.trim() || null,
                amount:
                  to === 'won' && amountValue.trim()
                    ? { value: amountValue.trim(), currency: currency || null, basis: 'accepted', source: 'user_stated' }
                    : null,
                expectedRevision: o.revision,
                idempotencyKey: key,
              }),
            )
          }
        >
          Move to {STAGE_LABELS[to]}
        </button>
      </div>
    </section>
  );
}

/** Records actual activity (user-reported); drafts and copies never count. */
function ActivityForm({ o, experiments }: { o: Opportunity; experiments: { id: string; label: string }[] }) {
  const [kind, setKind] = useState<ActivityType>('contact');
  const [today] = useState(() => toDateInput(Date.now()));
  const [date, setDate] = useState(today);
  const [person, setPerson] = useState('');
  const [detail, setDetail] = useState('');
  const [experimentId, setExperimentId] = useState('');
  const [key, setKey] = useState(() => requestKey('activity'));
  const [done, setDone] = useState<string | null>(null);
  const action = useAction();
  return (
    <section className="biz-opp__block" aria-label="Record activity">
      <h4 className="biz-detail__title">Record what actually happened</h4>
      <div className="biz-form-grid">
        <label className="field">
          <span className="field__label">Activity</span>
          <select className="input" value={kind} onChange={(e) => setKind(e.target.value as ActivityType)}>
            {RECORDABLE.map((k) => (
              <option key={k} value={k}>
                {ACTIVITY_LABELS[k]}
              </option>
            ))}
          </select>
        </label>
        <label className="field">
          <span className="field__label">When</span>
          <input className="input" type="date" max={today} value={date} onChange={(e) => setDate(e.target.value)} />
        </label>
        <label className="field">
          <span className="field__label">Person (optional)</span>
          <input className="input" value={person} onChange={(e) => setPerson(e.target.value)} />
        </label>
        {experiments.length > 0 && (
          <label className="field">
            <span className="field__label">Experiment (optional)</span>
            <select className="input" value={experimentId} onChange={(e) => setExperimentId(e.target.value)}>
              <option value="">None</option>
              {experiments.map((e) => (
                <option key={e.id} value={e.id}>
                  {e.label.slice(0, 60) || 'Experiment'}
                </option>
              ))}
            </select>
          </label>
        )}
      </div>
      <label className="field">
        <span className="field__label">Detail (optional)</span>
        <input className="input" value={detail} onChange={(e) => setDetail(e.target.value)} />
      </label>
      <p className="form__hint">
        Recorded as user-reported. A scheduled meeting is not a meeting held; a proposal draft is not a proposal sent;
        Won is not money received.
      </p>
      {action.error && (
        <p className="form-error" role="alert">
          {action.error}
        </p>
      )}
      <div className="biz-card__actions">
        {done && (
          <span className="biz-saved" role="status">
            {done}
          </span>
        )}
        <button
          type="button"
          className="button button--primary button--small"
          disabled={action.busy}
          onClick={() =>
            void action
              .run(async () => {
                const result = await recordActivity(o.id, {
                  kind,
                  person: person.trim() || null,
                  occurredAt: occurredAt(date),
                  detail: detail.trim() || null,
                  experimentId: experimentId || null,
                  idempotencyKey: key,
                });
                setDone(result.created ? 'Recorded.' : 'Already recorded.');
              })
              .then((ok) => {
                if (ok) {
                  setKey(requestKey('activity'));
                  setDetail('');
                }
              })
          }
        >
          Record
        </button>
      </div>
    </section>
  );
}

function History({ o }: { o: Opportunity }) {
  const action = useAction();
  const items = [...o.activities].sort((a, b) => b.occurredAt - a.occurredAt || b.recordedAt - a.recordedAt);
  return (
    <section className="biz-opp__block" aria-label="History">
      <h4 className="biz-detail__title">History</h4>
      {items.length === 0 && <p className="biz-muted">Nothing recorded yet.</p>}
      <ul className="biz-history">
        {items.map((a) => (
          <li key={a.id}>
            <span className="biz-history__date">{formatDate(a.occurredAt)}</span>
            <span className="biz-history__what">
              {a.kind === 'stage_change' && a.fromStage && a.toStage
                ? `Stage: ${STAGE_LABELS[a.fromStage]} → ${STAGE_LABELS[a.toStage]}`
                : ACTIVITY_LABELS[a.kind]}
              {a.person && ` · ${a.person}`}
              {a.variant && ` · variant ${a.variant}`}
              {a.detail && <span className="biz-muted"> — {a.detail}</span>}
            </span>
            <span className="biz-muted biz-history__source">{a.source === 'user_reported' ? 'user-reported' : a.source}</span>
            {a.kind !== 'stage_change' && (
              <button
                type="button"
                className="button button--ghost button--small"
                aria-label={`Delete ${ACTIVITY_LABELS[a.kind]} on ${formatDate(a.occurredAt)}`}
                disabled={action.busy}
                onClick={() => void action.run(() => deleteActivity(o.id, a.id))}
              >
                <TrashIcon className="button__icon" />
              </button>
            )}
          </li>
        ))}
      </ul>
      {action.error && (
        <p className="form-error" role="alert">
          {action.error}
        </p>
      )}
    </section>
  );
}

function Contacts({ o }: { o: Opportunity }) {
  const [confirm, setConfirm] = useState<{ id: string; kind: 'dnc' | 'delete' } | null>(null);
  const action = useAction();
  if (o.contacts.length === 0) {
    return <p className="biz-muted">No contact saved. Buyer roles appear here when research identifies them.</p>;
  }
  return (
    <section className="biz-opp__block" aria-label="Contacts">
      <ul className="biz-contacts">
        {o.contacts.map((c) => (
          <li key={c.id}>
            <span className="biz-person">{c.name ?? c.role}</span>
            {c.name && <span className="biz-muted"> · {c.title ?? c.role}</span>}
            <Links
              links={[
                c.profileUrl ? { label: 'Profile', url: c.profileUrl } : null,
                c.sourceUrl ? { label: 'Source', url: c.sourceUrl } : null,
              ].filter((l): l is { label: string; url: string } => l !== null)}
            />
            <span className="biz-contacts__actions">
              <button type="button" className="button button--ghost button--small" onClick={() => setConfirm({ id: c.id, kind: 'dnc' })}>
                Do not contact
              </button>
              <button type="button" className="button button--ghost button--small" onClick={() => setConfirm({ id: c.id, kind: 'delete' })}>
                Delete
              </button>
            </span>
            {confirm?.id === c.id && (
              <div
                className={confirm.kind === 'delete' ? 'notice notice--danger biz-confirm' : 'notice notice--warning biz-confirm'}
                role="alertdialog"
                aria-label={confirm.kind === 'delete' ? 'Delete contact' : 'Do not contact'}
              >
                <span>
                  {confirm.kind === 'delete'
                    ? 'Delete this contact? Drafts addressed to them are deleted and their name is removed from notes and history; a redaction record is kept.'
                    : 'Research and drafting will not bring this person back as a contact. Only you can lift it.'}
                </span>
                <button
                  type="button"
                  className={confirm.kind === 'delete' ? 'button button--danger button--small' : 'button button--primary button--small'}
                  onClick={() =>
                    void action
                      .run(() => (confirm.kind === 'delete' ? deleteContact(o.id, c.id, o.revision) : suppressContact(o.id, c.id, null)))
                      .then(() => setConfirm(null))
                  }
                >
                  Confirm
                </button>
                <button type="button" className="button button--ghost button--small" onClick={() => setConfirm(null)}>
                  Cancel
                </button>
              </div>
            )}
          </li>
        ))}
      </ul>
      {action.error && (
        <p className="form-error" role="alert">
          {action.error}
        </p>
      )}
    </section>
  );
}
