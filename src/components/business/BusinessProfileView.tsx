import { useState } from 'react';

import { useAction } from '../../hooks/useAction';
import { useBusinessRun } from '../../hooks/useBusiness';
import {
  createOffer,
  deleteOffer,
  describeOffer,
  describeOfferFromDocument,
  requestKey,
  saveBusinessProfile,
  type BusinessOverview,
  type BusinessProfile,
  type DescribeInput,
  type DescribeResult,
  type Offer,
  type OfferKind,
} from '../../services/businessService';
import { ArrowLeftIcon, FileIcon, GlobeIcon, PencilIcon, PlusIcon, TrashIcon } from '../icons';
import { ChipInput } from '../profile/EntryList';
import { RunProgress } from './common';
import { MATURITY_LABELS, OFFER_KIND_LABELS } from './labels';
import { emptyOfferContent, hasUnreviewedChanges, reveal } from './helpers';
import { OfferEditor } from './OfferEditor';

type Source = 'url' | 'text' | 'document' | 'manual';

const SOURCES: { id: Source; label: string }[] = [
  { id: 'url', label: 'From a website' },
  { id: 'text', label: 'From a description' },
  { id: 'document', label: 'From a document' },
  { id: 'manual', label: 'By hand' },
];

/**
 * The Business Profile: what the user is prepared to sell (B3), separate
 * from the career Profile. Offers become usable for research only after
 * the user saves a reviewed version.
 */
export function BusinessProfileView({
  overview,
  onBack,
  initialOfferId,
}: {
  overview: BusinessOverview;
  onBack: () => void;
  initialOfferId: string | null;
}) {
  const [editing, setEditing] = useState<string | null>(initialOfferId);
  const [report, setReport] = useState<DescribeResult | null>(null);
  const offer = overview.offers.find((o) => o.id === editing) ?? null;

  if (offer) {
    return (
      <div className="biz-profile">
        <button type="button" className="button button--ghost button--small biz-back" onClick={() => setEditing(null)}>
          <ArrowLeftIcon className="button__icon" />
          Business Profile
        </button>
        <OfferEditor
          key={offer.id}
          offer={offer}
          report={report?.offer.id === offer.id ? report : null}
          onReport={setReport}
          onClose={() => {
            setEditing(null);
            setReport(null);
          }}
        />
      </div>
    );
  }

  return (
    <div className="biz-profile">
      <button type="button" className="button button--ghost button--small biz-back" onClick={onBack}>
        <ArrowLeftIcon className="button__icon" />
        Back to Business
      </button>
      <ProfileForm key={overview.profile.updatedAt} profile={overview.profile} />
      <OfferList
        offers={overview.offers}
        onEdit={setEditing}
        onCreated={(offerId, result) => {
          setReport(result);
          setEditing(offerId);
        }}
      />
    </div>
  );
}

function ProfileForm({ profile }: { profile: BusinessProfile }) {
  const [draft, setDraft] = useState(profile);
  const [saved, setSaved] = useState(false);
  const action = useAction();
  const set = (patch: Partial<BusinessProfile>) => {
    setDraft({ ...draft, ...patch });
    setSaved(false);
  };
  const text = (key: 'businessName' | 'website' | 'serviceArea' | 'capacity' | 'availability', label: string, placeholder: string) => (
    <label className="field">
      <span className="field__label">{label}</span>
      <input className="input" placeholder={placeholder} value={draft[key]} onChange={(e) => set({ [key]: e.target.value })} />
    </label>
  );
  return (
    <section className="biz-card" aria-label="Your business">
      <h2 className="biz-section-title">Your business</h2>
      <p className="biz-muted">
        All optional. ReMa never needs a company registration, tax number or bank details for research.
      </p>
      <div className="biz-form-grid">
        {text('businessName', 'Business name', 'Your company or trading name')}
        {text('website', 'Website', 'https://…')}
        {text('serviceArea', 'Service area', 'e.g. DACH, remote within the EU')}
        <div className="field">
          <span className="field__label">Working languages</span>
          <ChipInput
            label="Working languages"
            placeholder="German, English…"
            values={draft.languages}
            onChange={(languages) => set({ languages })}
          />
        </div>
        {text('capacity', 'Delivery capacity', 'e.g. two projects at a time')}
        {text('availability', 'Availability', 'e.g. from November, 3 days a week')}
      </div>
      <label className="field">
        <span className="field__label">Commercial constraints</span>
        <textarea
          className="input input--textarea"
          rows={2}
          placeholder="e.g. no on-site work outside Austria; minimum engagement two weeks"
          value={draft.constraints}
          onChange={(e) => set({ constraints: e.target.value })}
        />
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
        <button
          type="button"
          className="button button--primary"
          disabled={action.busy}
          onClick={() => void action.run(() => saveBusinessProfile(draft)).then((ok) => setSaved(ok))}
        >
          Save
        </button>
      </div>
    </section>
  );
}

function offerState(o: Offer): { label: string; className: string } {
  if (o.archived) return { label: 'Archived', className: 'badge' };
  if (o.currentVersion === null) return { label: 'Not reviewed', className: 'badge badge--warning' };
  if (hasUnreviewedChanges(o)) {
    return { label: `Reviewed v${o.currentVersion} · unreviewed changes`, className: 'badge badge--brand' };
  }
  return { label: `Reviewed v${o.currentVersion}`, className: 'badge badge--success' };
}

function OfferList({
  offers,
  onEdit,
  onCreated,
}: {
  offers: Offer[];
  onEdit: (id: string) => void;
  onCreated: (offerId: string, result: DescribeResult | null) => void;
}) {
  const [adding, setAdding] = useState(offers.length === 0);
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);
  const action = useAction();
  return (
    <section className="biz-card" aria-label="Offers">
      <div className="biz-card__head">
        <h2 className="biz-section-title">Offers</h2>
        {!adding && (
          <button type="button" className="button button--secondary button--small" onClick={() => setAdding(true)}>
            <PlusIcon className="button__icon" />
            New offer
          </button>
        )}
      </div>
      <p className="biz-muted">
        What you sell: a service, a digital product or both. Find Clients and the Go-to-Market Studio use the
        reviewed version you select; each research run keeps the version it used.
      </p>
      {offers.length > 0 && (
        <ul className="biz-offers">
          {offers.map((o) => {
            const state = offerState(o);
            const content = o.draft ?? o.reviewed;
            return (
              <li key={o.id} className="biz-offers__item">
                <div className="biz-offers__main">
                  <button type="button" className="link-button biz-offers__name" onClick={() => onEdit(o.id)}>
                    {o.name}
                  </button>
                  <span className="biz-muted">
                    {OFFER_KIND_LABELS[o.kind]}
                    {content && content.maturity !== 'not_stated' && ` · ${MATURITY_LABELS[content.maturity]}`}
                  </span>
                  {content?.summary.text && <span className="biz-offers__summary">{content.summary.text}</span>}
                </div>
                <span className={state.className}>{state.label}</span>
                <div className="biz-offers__actions">
                  <button type="button" className="button button--ghost button--small" onClick={() => onEdit(o.id)}>
                    <PencilIcon className="button__icon" />
                    {o.currentVersion === null ? 'Review' : 'Edit'}
                  </button>
                  <button
                    type="button"
                    className="button button--ghost button--small"
                    aria-label={`Delete ${o.name}`}
                    onClick={() => setConfirmDelete(o.id)}
                  >
                    <TrashIcon className="button__icon" />
                  </button>
                </div>
                {confirmDelete === o.id && (
                  <div ref={reveal} className="notice notice--danger biz-confirm" role="alertdialog" aria-label={`Delete ${o.name}`}>
                    <span>
                      Delete “{o.name}” and its versions? Earlier research keeps its record as “Deleted offer”; saved
                      opportunities stay in the Pipeline.
                    </span>
                    <button
                      type="button"
                      className="button button--danger button--small"
                      disabled={action.busy}
                      onClick={() => void action.run(() => deleteOffer(o.id)).then(() => setConfirmDelete(null))}
                    >
                      Delete
                    </button>
                    <button type="button" className="button button--ghost button--small" onClick={() => setConfirmDelete(null)}>
                      Cancel
                    </button>
                  </div>
                )}
              </li>
            );
          })}
        </ul>
      )}
      {action.error && (
        <p className="form-error" role="alert">
          {action.error}
        </p>
      )}
      {adding && <NewOffer onCreated={onCreated} onCancel={offers.length > 0 ? () => setAdding(false) : null} />}
    </section>
  );
}

/**
 * A new offer from a website, a description, a document or by hand (B4).
 * Only a read produces a report: an offer entered by hand has nothing that
 * ReMa read.
 */
function NewOffer({
  onCreated,
  onCancel,
}: {
  onCreated: (offerId: string, result: DescribeResult | null) => void;
  onCancel: (() => void) | null;
}) {
  const [source, setSource] = useState<Source>('url');
  const [name, setName] = useState('');
  const [kind, setKind] = useState<OfferKind>('service');
  const [url, setUrl] = useState('');
  const [text, setText] = useState('');
  const [key] = useState(() => requestKey('offer'));
  const run = useBusinessRun();
  const action = useAction();

  const input = (runId: string): DescribeInput => ({
    name: name.trim(),
    kind,
    url: source === 'url' ? url.trim() : null,
    text: source === 'text' ? text : null,
    documentPath: null,
    offerId: null,
    runId,
    idempotencyKey: key,
  });

  const submit = () => {
    if (source === 'manual') {
      void action.run(async () => {
        const offer = await createOffer(emptyOfferContent(name.trim(), kind), key);
        onCreated(offer.id, null);
      });
      return;
    }
    void run.start(async (runId) => {
      const result =
        source === 'document' ? await describeOfferFromDocument(input(runId)) : await describeOffer(input(runId));
      if (result) onCreated(result.offer.id, result);
      return result;
    });
  };

  const ready =
    name.trim() !== '' &&
    (source === 'url' ? url.trim() !== '' : source === 'text' ? text.trim() !== '' : true);

  return (
    <div className="biz-new-offer">
      <div className="segmented" role="radiogroup" aria-label="Start from">
        {SOURCES.map((s) => (
          <button
            key={s.id}
            type="button"
            role="radio"
            aria-checked={source === s.id}
            className={source === s.id ? 'segmented__option segmented__option--active' : 'segmented__option'}
            onClick={() => setSource(s.id)}
          >
            {s.label}
          </button>
        ))}
      </div>
      <div className="biz-form-grid">
        <label className="field">
          <span className="field__label">Offer name</span>
          <input className="input" value={name} placeholder="e.g. Support Workspace" onChange={(e) => setName(e.target.value)} />
        </label>
        <label className="field">
          <span className="field__label">Type</span>
          <select className="input" value={kind} onChange={(e) => setKind(e.target.value as OfferKind)}>
            {(Object.keys(OFFER_KIND_LABELS) as OfferKind[]).map((k) => (
              <option key={k} value={k}>
                {OFFER_KIND_LABELS[k]}
              </option>
            ))}
          </select>
        </label>
      </div>
      {source === 'url' && (
        <label className="field">
          <span className="field__label">Product or service page</span>
          <input
            className="input"
            type="url"
            placeholder="https://example.com/product"
            value={url}
            onChange={(e) => setUrl(e.target.value)}
          />
          <span className="form__hint">
            ReMa reads up to eight public pages of this site (features, pricing, integrations, use cases). It never
            logs in, submits forms or follows instructions on the pages. You review every claim before it is used.
          </span>
        </label>
      )}
      {source === 'text' && (
        <label className="field">
          <span className="field__label">Description</span>
          <textarea
            className="input input--textarea"
            rows={5}
            placeholder="What it does, the problem it solves, who it is for, how it is delivered…"
            value={text}
            onChange={(e) => setText(e.target.value)}
          />
        </label>
      )}
      {source === 'document' && (
        <p className="biz-muted">
          <FileIcon className="biz-inline-icon" aria-hidden="true" /> Choose a PDF, Word, text or Markdown file in the
          next step. ReMa reads it into a draft for your review.
        </p>
      )}
      {source === 'manual' && (
        <p className="biz-muted">Start with an empty offer and fill in what it does and the problem it addresses.</p>
      )}
      {run.running && <RunProgress status={run.status} fallback="Reading…" onStop={run.stop} />}
      {(run.error || action.error) && (
        <p className="form-error" role="alert">
          {run.error ?? action.error}
        </p>
      )}
      <div className="biz-card__actions">
        {onCancel && (
          <button type="button" className="button button--ghost" onClick={onCancel} disabled={run.running}>
            Cancel
          </button>
        )}
        <button type="button" className="button button--primary" disabled={!ready || run.running || action.busy} onClick={submit}>
          {source === 'url' && <GlobeIcon className="button__icon" />}
          {source === 'url' ? 'Read website' : source === 'document' ? 'Choose document…' : source === 'text' ? 'Read description' : 'Create offer'}
        </button>
      </div>
    </div>
  );
}
