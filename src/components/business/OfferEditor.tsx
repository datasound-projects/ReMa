import { useState } from 'react';

import { useAction } from '../../hooks/useAction';
import { useBusinessRun } from '../../hooks/useBusiness';
import { formatDateTime } from '../../lib/format';
import {
  archiveOffer,
  describeOffer,
  reviewOffer,
  saveOfferDraft,
  type Claim,
  type DescribeResult,
  type FieldStatus,
  type Maturity,
  type Offer,
  type OfferContent,
  type OfferKind,
  type PricePoint,
  type PriceUnit,
} from '../../services/businessService';
import { CheckIcon, CloseIcon, PlusIcon, ReloadIcon } from '../icons';
import { ChipInput } from '../profile/EntryList';
import { Links, RunProgress, StatusBadge } from './common';
import { emptyOfferContent, openLink, reviewBlockers } from './helpers';
import {
  FIELD_STATUS_LABELS,
  MATURITY_LABELS,
  OFFER_KIND_LABELS,
  PAGE_STATUS_LABELS,
  PRICE_UNIT_LABELS,
} from './labels';

type ListField =
  | 'outcomes'
  | 'features'
  | 'useCases'
  | 'customerTypes'
  | 'buyerRoles'
  | 'geography'
  | 'languages'
  | 'requirements'
  | 'integrations'
  | 'deploymentConstraints'
  | 'exclusions'
  | 'limitations';

const LIST_FIELDS: { field: ListField; label: string; hint?: string }[] = [
  { field: 'outcomes', label: 'Intended outcomes', hint: 'Performance claims stay marketing claims until you prove them.' },
  { field: 'features', label: 'Features or deliverables' },
  { field: 'useCases', label: 'Use cases' },
  { field: 'customerTypes', label: 'Intended customer types' },
  { field: 'buyerRoles', label: 'Proposed buyer roles' },
  { field: 'integrations', label: 'Integrations' },
  { field: 'requirements', label: 'Requirements' },
  { field: 'deploymentConstraints', label: 'Deployment constraints' },
  { field: 'geography', label: 'Geographic availability' },
  { field: 'languages', label: 'Languages' },
  { field: 'exclusions', label: 'Exclusions (who it is not for)' },
  { field: 'limitations', label: 'Known limitations' },
];

type SingleField = 'summary' | 'problem' | 'deliveryModel';

const SINGLE_FIELDS: { field: SingleField; label: string; required?: boolean }[] = [
  { field: 'summary', label: 'What it does', required: true },
  { field: 'problem', label: 'Problem it addresses', required: true },
  { field: 'deliveryModel', label: 'Delivery model' },
];

function userClaim(text: string): Claim {
  return { text, status: 'user_confirmed', sourceUrl: null, retrievedAt: null, excerpt: null, note: null };
}

function StatusTag({ status }: { status: FieldStatus }) {
  const tone =
    status === 'user_confirmed'
      ? 'badge badge--success'
      : status === 'conflicting'
        ? 'badge badge--danger'
        : status === 'observed' || status === 'hypothesis'
          ? 'badge badge--warning'
          : 'badge';
  return <span className={tone}>{FIELD_STATUS_LABELS[status]}</span>;
}

/** Where a claim came from: its page, retrieval time and supporting excerpt. */
function Provenance({ claim }: { claim: Claim }) {
  if (!claim.sourceUrl && !claim.excerpt && !claim.note) return null;
  return (
    <div className="biz-claim__provenance">
      {claim.sourceUrl && (
        <button type="button" className="link-button" onClick={() => openLink(claim.sourceUrl)}>
          {claim.sourceUrl.replace(/^https?:\/\//, '')}
        </button>
      )}
      {claim.retrievedAt !== null && <span> · retrieved {formatDateTime(claim.retrievedAt)}</span>}
      {claim.excerpt && <span className="biz-claim__excerpt">“{claim.excerpt}”</span>}
      {claim.note && <span className="biz-claim__note">{claim.note}</span>}
    </div>
  );
}

/** One claim: edit (→ confirmed by you), confirm, or reject as unsupported. */
function ClaimRow({
  claim,
  label,
  multiline,
  onChange,
  onReject,
  onRemove,
}: {
  claim: Claim;
  label: string;
  multiline?: boolean;
  onChange: (next: Claim) => void;
  onReject?: () => void;
  onRemove?: () => void;
}) {
  const edit = (text: string) =>
    onChange({
      ...claim,
      text,
      status: text.trim() ? 'user_confirmed' : 'unknown',
      note: claim.sourceUrl ? 'Corrected by you.' : claim.note,
    });
  return (
    <div className={`biz-claim biz-claim--${claim.status}`}>
      <div className="biz-claim__row">
        {multiline ? (
          <textarea
            className="input input--textarea biz-claim__input"
            aria-label={label}
            rows={2}
            value={claim.text}
            placeholder="Unknown"
            onChange={(e) => edit(e.target.value)}
          />
        ) : (
          <input
            className="input biz-claim__input"
            aria-label={label}
            value={claim.text}
            placeholder="Unknown"
            onChange={(e) => edit(e.target.value)}
          />
        )}
        <div className="biz-claim__actions">
          <StatusTag status={claim.status} />
          {claim.status !== 'user_confirmed' && claim.text.trim() !== '' && (
            <button
              type="button"
              className="button button--ghost button--small"
              title="Confirm this claim"
              onClick={() => onChange({ ...claim, status: 'user_confirmed' })}
            >
              <CheckIcon className="button__icon" />
              Confirm
            </button>
          )}
          {onReject && claim.text.trim() !== '' && (
            <button
              type="button"
              className="button button--ghost button--small"
              title="Not true or not supported: keep it out of this offer"
              onClick={onReject}
            >
              Reject
            </button>
          )}
          {onRemove && (
            <button
              type="button"
              className="button button--ghost button--small"
              aria-label={`Remove ${label}`}
              onClick={onRemove}
            >
              <CloseIcon className="button__icon" />
            </button>
          )}
        </div>
      </div>
      <Provenance claim={claim} />
    </div>
  );
}

function PriceRow({
  price,
  onChange,
  onRemove,
}: {
  price: PricePoint;
  onChange: (next: PricePoint) => void;
  onRemove: () => void;
}) {
  const confirm = (patch: Partial<PricePoint>) =>
    onChange({ ...price, ...patch, claim: { ...price.claim, status: 'user_confirmed' } });
  return (
    <div className={`biz-claim biz-claim--${price.claim.status}`}>
      <div className="biz-claim__row biz-price">
        <input
          className="input biz-price__amount"
          aria-label="Amount or range"
          placeholder="Amount or range"
          value={price.amount}
          onChange={(e) => confirm({ amount: e.target.value })}
        />
        <input
          className="input biz-price__currency"
          aria-label="Currency"
          placeholder="EUR"
          maxLength={3}
          value={price.currency ?? ''}
          onChange={(e) => confirm({ currency: e.target.value.toUpperCase() || null })}
        />
        <select
          className="input biz-price__unit"
          aria-label="Price unit"
          value={price.unit}
          onChange={(e) => confirm({ unit: e.target.value as PriceUnit })}
        >
          {(Object.keys(PRICE_UNIT_LABELS) as PriceUnit[]).map((u) => (
            <option key={u} value={u}>
              {PRICE_UNIT_LABELS[u]}
            </option>
          ))}
        </select>
        <div className="biz-claim__actions">
          <StatusTag status={price.claim.status} />
          {price.claim.status !== 'user_confirmed' && (
            <button type="button" className="button button--ghost button--small" onClick={() => confirm({})}>
              <CheckIcon className="button__icon" />
              Confirm
            </button>
          )}
          <button type="button" className="button button--ghost button--small" aria-label="Remove price" onClick={onRemove}>
            <CloseIcon className="button__icon" />
          </button>
        </div>
      </div>
      <Provenance claim={price.claim} />
    </div>
  );
}

/** What the last website/description read produced (B4). */
function IngestReport({ report }: { report: DescribeResult }) {
  const diff = report.diff;
  return (
    <section className="biz-report" aria-label="What ReMa read">
      <div className="biz-report__head">
        <strong>What ReMa read</strong>
        <StatusBadge status={report.status} />
      </div>
      {report.pages.length > 0 && (
        <div className="table-wrap">
          <table className="data-table">
            <thead>
              <tr>
                <th scope="col">Page</th>
                <th scope="col">Status</th>
                <th scope="col">Text</th>
              </tr>
            </thead>
            <tbody>
              {report.pages.map((p) => (
                <tr key={p.url}>
                  <td>
                    <button type="button" className="link-button" onClick={() => openLink(p.url)}>
                      {p.title || p.url.replace(/^https?:\/\//, '')}
                    </button>
                  </td>
                  <td>
                    {PAGE_STATUS_LABELS[p.status]}
                    {p.detail && <span className="biz-muted"> · {p.detail}</span>}
                  </td>
                  <td className="biz-num">{p.characters.toLocaleString()} characters</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      {report.missing.length > 0 && (
        <p className="biz-report__missing">
          <strong>Still missing:</strong> {report.missing.join('; ')}
        </p>
      )}
      {report.notes.length > 0 && (
        <ul className="biz-notes">
          {report.notes.map((n) => (
            <li key={n}>{n}</li>
          ))}
        </ul>
      )}
      {diff && (
        <div className="biz-diff" aria-label="Changes found at refresh">
          <strong>Refresh proposal</strong> — nothing is saved until you save a reviewed version.
          {diff.added.length > 0 && <DiffList title="New on the website" items={diff.added} />}
          {diff.notFound.length > 0 && <DiffList title="No longer found on the website" items={diff.notFound} />}
          {diff.conflicts.length > 0 && <DiffList title="Conflicts with what you confirmed" items={diff.conflicts} />}
          {diff.stillRejected.length > 0 && (
            <DiffList title="Still on the website, still rejected by you" items={diff.stillRejected} />
          )}
          {diff.added.length + diff.notFound.length + diff.conflicts.length + diff.stillRejected.length === 0 && (
            <p className="biz-muted">No differences found.</p>
          )}
        </div>
      )}
    </section>
  );
}

function DiffList({ title, items }: { title: string; items: string[] }) {
  return (
    <div className="biz-diff__group">
      <span className="biz-diff__title">{title}</span>
      <ul>
        {items.map((i) => (
          <li key={i}>{i}</li>
        ))}
      </ul>
    </div>
  );
}

/**
 * The offer editor: every field shows its status and source; the user
 * confirms, corrects or rejects claims, then saves a reviewed version.
 * Research only ever uses reviewed versions.
 */
export function OfferEditor({
  offer,
  report,
  onReport,
  onClose,
}: {
  offer: Offer;
  report: DescribeResult | null;
  onReport: (report: DescribeResult) => void;
  onClose: () => void;
}) {
  const [content, setContent] = useState<OfferContent>(() => offer.draft ?? offer.reviewed ?? emptyOfferContent(offer.name, offer.kind));
  const [revision, setRevision] = useState(offer.revision);
  const [dirty, setDirty] = useState(false);
  const [saved, setSaved] = useState<string | null>(null);
  const action = useAction();
  const refresh = useBusinessRun();
  const blockers = reviewBlockers(content);
  // A saved draft that is not a reviewed version yet.
  const [hasUnreviewedDraft, setHasUnreviewedDraft] = useState(offer.draft !== null);

  const update = (patch: Partial<OfferContent>) => {
    setContent((c) => ({ ...c, ...patch }));
    setDirty(true);
    setSaved(null);
  };

  const saveDraft = async () => {
    const next = await saveOfferDraft(offer.id, content, revision);
    setRevision(next.revision);
    setDirty(false);
    setHasUnreviewedDraft(true);
    return next;
  };

  const review = () =>
    void action.run(async () => {
      let rev = revision;
      if (dirty || !hasUnreviewedDraft) rev = (await saveDraft()).revision;
      const next = await reviewOffer(offer.id, rev);
      setRevision(next.revision);
      setDirty(false);
      setHasUnreviewedDraft(false);
      setSaved(`Saved as reviewed version ${next.currentVersion ?? ''}.`);
    });

  const refreshFromWebsite = () => {
    const url = content.websiteUrls[0];
    if (!url) return;
    void refresh.start(async (runId) => {
      if (dirty) await saveDraft();
      const result = await describeOffer({
        name: content.name,
        kind: content.kind,
        url,
        text: null,
        documentPath: null,
        offerId: offer.id,
        runId,
        idempotencyKey: null,
      });
      onReport(result);
      setContent(result.offer.draft ?? result.offer.reviewed ?? content);
      setRevision(result.offer.revision);
      setDirty(false);
      setHasUnreviewedDraft(result.offer.draft !== null);
      return result;
    });
  };

  const setList = (field: ListField, next: Claim[]) => update({ [field]: next } as Partial<OfferContent>);

  const reject = (claim: Claim, remove: () => void) => {
    remove();
    setContent((c) => ({ ...c, unsupportedClaims: [...c.unsupportedClaims, { ...claim, note: 'Rejected by you.' }] }));
    setDirty(true);
  };

  return (
    <div className="biz-offer-editor">
      <div className="biz-offer-editor__head">
        <div>
          <h2 className="biz-section-title">{content.name || 'New offer'}</h2>
          <p className="biz-muted">
            {offer.currentVersion !== null
              ? `Reviewed version ${offer.currentVersion}${offer.reviewedAt ? ` · ${formatDateTime(offer.reviewedAt)}` : ''}`
              : 'Not reviewed yet: research cannot use it until you save a reviewed version.'}
            {hasUnreviewedDraft && offer.currentVersion !== null && ' · Unreviewed changes'}
            {offer.archived && ' · Archived'}
          </p>
        </div>
        <div className="biz-offer-editor__head-actions">
          {content.websiteUrls.length > 0 && !refresh.running && (
            <button type="button" className="button button--secondary button--small" onClick={refreshFromWebsite}>
              <ReloadIcon className="button__icon" />
              Refresh from website
            </button>
          )}
          <button
            type="button"
            className="button button--ghost button--small"
            onClick={() =>
              void action.run(async () => {
                const next = await archiveOffer(offer.id, !offer.archived, revision);
                setRevision(next.revision);
              })
            }
          >
            {offer.archived ? 'Restore offer' : 'Archive offer'}
          </button>
          <button type="button" className="button button--ghost button--small" onClick={onClose}>
            Done
          </button>
        </div>
      </div>

      {refresh.running && (
        <RunProgress status={refresh.status} fallback="Reading the website…" onStop={refresh.stop} />
      )}
      {refresh.error && (
        <p className="form-error" role="alert">
          {refresh.error}
        </p>
      )}
      {report && <IngestReport report={report} />}

      <div className="biz-form-grid">
        <label className="field">
          <span className="field__label">Name</span>
          <input className="input" value={content.name} onChange={(e) => update({ name: e.target.value })} />
        </label>
        <label className="field">
          <span className="field__label">Type</span>
          <select className="input" value={content.kind} onChange={(e) => update({ kind: e.target.value as OfferKind })}>
            {(Object.keys(OFFER_KIND_LABELS) as OfferKind[]).map((k) => (
              <option key={k} value={k}>
                {OFFER_KIND_LABELS[k]}
              </option>
            ))}
          </select>
        </label>
        <label className="field">
          <span className="field__label">Maturity (your choice)</span>
          <select
            className="input"
            value={content.maturity}
            onChange={(e) => update({ maturity: e.target.value as Maturity })}
          >
            {(Object.keys(MATURITY_LABELS) as Maturity[]).map((m) => (
              <option key={m} value={m}>
                {MATURITY_LABELS[m]}
              </option>
            ))}
          </select>
        </label>
      </div>

      {SINGLE_FIELDS.map(({ field, label, required }) => (
        <div key={field} className="biz-field">
          <span className="field__label">
            {label}
            {required && <span className="biz-required"> · required</span>}
          </span>
          <ClaimRow claim={content[field]} label={label} multiline onChange={(next) => update({ [field]: next })} />
        </div>
      ))}

      {LIST_FIELDS.map(({ field, label, hint }) => (
        <div key={field} className="biz-field">
          <span className="field__label">{label}</span>
          {hint && <span className="form__hint">{hint}</span>}
          {content[field].length === 0 && <span className="biz-muted">Unknown — nothing stated yet.</span>}
          {content[field].map((claim, index) => (
            <ClaimRow
              key={index}
              claim={claim}
              label={`${label} ${index + 1}`}
              onChange={(next) => setList(field, content[field].map((c, i) => (i === index ? next : c)))}
              onReject={() => reject(claim, () => setList(field, content[field].filter((_, i) => i !== index)))}
              onRemove={() => setList(field, content[field].filter((_, i) => i !== index))}
            />
          ))}
          <button
            type="button"
            className="button button--ghost button--small biz-field__add"
            onClick={() => setList(field, [...content[field], userClaim('')])}
          >
            <PlusIcon className="button__icon" />
            Add
          </button>
        </div>
      ))}

      <div className="biz-field">
        <span className="field__label">Pricing (optional)</span>
        <span className="form__hint">Units stay as stated: a monthly subscription is never compared with a daily rate.</span>
        {content.pricing.map((price, index) => (
          <PriceRow
            key={index}
            price={price}
            onChange={(next) => update({ pricing: content.pricing.map((p, i) => (i === index ? next : p)) })}
            onRemove={() => update({ pricing: content.pricing.filter((_, i) => i !== index) })}
          />
        ))}
        <button
          type="button"
          className="button button--ghost button--small biz-field__add"
          onClick={() =>
            update({
              pricing: [...content.pricing, { amount: '', currency: 'EUR', unit: 'daily', claim: userClaim('') }],
            })
          }
        >
          <PlusIcon className="button__icon" />
          Add price
        </button>
      </div>

      <div className="biz-field">
        <span className="field__label">Website pages</span>
        <ChipInput
          label="Website pages"
          placeholder="https://…"
          values={content.websiteUrls}
          onChange={(websiteUrls) => update({ websiteUrls })}
        />
        {content.websiteUrls.length > 0 && (
          <Links links={content.websiteUrls.map((u) => ({ label: u.replace(/^https?:\/\//, ''), url: u }))} />
        )}
      </div>

      {content.unsupportedClaims.length > 0 && (
        <div className="biz-field">
          <span className="field__label">Rejected claims</span>
          <span className="form__hint">Kept so a website refresh cannot bring them back unnoticed.</span>
          <ul className="biz-rejected">
            {content.unsupportedClaims.map((c, index) => (
              <li key={index}>
                <span>{c.text}</span>
                <button
                  type="button"
                  className="link-button"
                  onClick={() => update({ unsupportedClaims: content.unsupportedClaims.filter((_, i) => i !== index) })}
                >
                  Forget
                </button>
              </li>
            ))}
          </ul>
        </div>
      )}

      <div className="biz-offer-editor__footer">
        {blockers.length > 0 && (
          <ul className="biz-blockers" aria-label="Before you can save a reviewed version">
            {blockers.map((b) => (
              <li key={b}>{b}</li>
            ))}
          </ul>
        )}
        {action.error && (
          <p className="form-error" role="alert">
            {action.error}
          </p>
        )}
        {saved && (
          <p className="biz-saved" role="status">
            {saved}
          </p>
        )}
        <div className="biz-offer-editor__buttons">
          <button
            type="button"
            className="button button--secondary"
            disabled={!dirty || action.busy}
            onClick={() => void action.run(saveDraft).then((ok) => ok && setSaved('Draft saved.'))}
          >
            Save draft
          </button>
          <button
            type="button"
            className="button button--primary"
            disabled={blockers.length > 0 || action.busy || (!dirty && !hasUnreviewedDraft && offer.currentVersion !== null)}
            onClick={review}
          >
            Save reviewed version
          </button>
        </div>
      </div>
    </div>
  );
}
