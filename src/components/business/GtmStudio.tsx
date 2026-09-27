import { useState, type ReactNode } from 'react';

import { useAction } from '../../hooks/useAction';
import { dataOr } from '../../hooks/useAsyncData';
import { useBusinessRun, usePipeline } from '../../hooks/useBusiness';
import { formatDateTime } from '../../lib/format';
import {
  createPlan,
  deletePlan,
  draftPositioning,
  findTargetAccounts,
  requestKey,
  researchPlan,
  savePlan,
  saveProspect,
  type Alternative,
  type BusinessOverview,
  type ChannelPlan,
  type GtmPlan,
  type Offer,
  type PlanContent,
  type PlanPart,
  type Segment,
  type SegmentStatus,
} from '../../services/businessService';
import { PlusIcon, SparkleIcon, TrashIcon } from '../icons';
import { EmptyState } from '../ui/EmptyState';
import { FitCell, Links, RunProgress, SourceNotes } from './common';
import { DraftCard, NewDraftForm } from './Drafts';
import { Experiments } from './Experiments';
import { reveal } from './helpers';
import { SEGMENT_STATUS_LABELS } from './labels';

type Step = 'segments' | 'alternatives' | 'channels' | 'accounts' | 'drafts' | 'experiments';

const STEPS: { id: Step; label: string }[] = [
  { id: 'segments', label: 'Segments' },
  { id: 'alternatives', label: 'Alternatives & positioning' },
  { id: 'channels', label: 'Channels' },
  { id: 'accounts', label: 'Target accounts' },
  { id: 'drafts', label: 'Drafts' },
  { id: 'experiments', label: 'Experiments' },
];

/** A textarea edited as one item per line; keeps its own text while typing. */
function LinesField({
  label,
  value,
  onChange,
  rows = 2,
  placeholder,
}: {
  label: string;
  value: string[];
  onChange: (next: string[]) => void;
  rows?: number;
  placeholder?: string;
}) {
  const [text, setText] = useState(value.join('\n'));
  return (
    <label className="field">
      <span className="field__label">{label}</span>
      <textarea
        className="input input--textarea"
        rows={rows}
        placeholder={placeholder ?? 'One per line'}
        value={text}
        onChange={(e) => {
          setText(e.target.value);
          onChange(
            e.target.value
              .split('\n')
              .map((l) => l.trim())
              .filter(Boolean),
          );
        }}
      />
    </label>
  );
}

function blankSegment(geography: string): Segment {
  return {
    id: '',
    name: '',
    organizationType: '',
    geography,
    sizeBand: '',
    useCase: '',
    painHypothesis: '',
    prerequisites: [],
    buyerRoles: [],
    likelyObjections: [],
    observableSignals: [],
    disqualifiers: [],
    supportingEvidence: [],
    counterevidence: [],
    unknowns: [],
    validationQuestions: [],
    status: 'hypothesis',
    selected: false,
  };
}

function blankChannel(): ChannelPlan {
  return {
    name: '',
    audience: '',
    why: '',
    entryPoint: '',
    rules: '',
    effort: '',
    costs: '',
    unknowns: [],
    test: '',
    available: true,
    evidence: [],
  };
}

/**
 * Go-to-Market Studio (B17): views over one plan for the selected reviewed
 * offer — segments, alternatives and positioning, channels, target
 * accounts, drafts and experiments. It plans; it never sends or spends.
 */
export function GtmStudio({
  overview,
  offer,
  onEditOffer,
  onOpenOpportunity,
}: {
  overview: BusinessOverview;
  offer: Offer | null;
  onEditOffer: () => void;
  onOpenOpportunity: (id: string) => void;
}) {
  const plans = offer ? overview.plans.filter((p) => p.offer.offerId === offer.id) : [];
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const plan = plans.find((p) => p.id === selectedId) ?? plans[0] ?? null;
  const [creating, setCreating] = useState(false);
  // Outside the editor, which is remounted when the plan changes on disk.
  const [step, setStep] = useState<Step>('segments');

  if (!offer || offer.currentVersion === null || offer.archived) {
    return (
      <EmptyState
        title="Start from a reviewed offer"
        framed
        actions={
          <button type="button" className="button button--primary" onClick={onEditOffer}>
            {offer ? 'Open offer' : 'Describe an offer'}
          </button>
        }
      >
        The Go-to-Market Studio works on a reviewed version of what you sell: who might buy it, against which
        alternatives, through which channels, and which small test to run next.
      </EmptyState>
    );
  }

  return (
    <div className="biz-view">
      <div className="biz-toolbar">
        {plans.length > 0 && (
          <label className="biz-toolbar__label">
            <span className="field__label">Plan</span>
            <select className="input biz-toolbar__select" value={plan?.id ?? ''} onChange={(e) => setSelectedId(e.target.value)}>
              {plans.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.name} ({p.offer.name} v{p.offer.version})
                </option>
              ))}
            </select>
          </label>
        )}
        <span className="biz-search__spacer" />
        {!creating && plans.length > 0 && (
          <button type="button" className="button button--secondary button--small" onClick={() => setCreating(true)}>
            <PlusIcon className="button__icon" />
            New plan
          </button>
        )}
      </div>
      {(creating || plans.length === 0) && (
        <NewPlan
          offer={offer}
          onCreated={(p) => {
            setSelectedId(p.id);
            setCreating(false);
          }}
          onCancel={plans.length > 0 ? () => setCreating(false) : null}
        />
      )}
      {plan && (
        <PlanEditor
          key={`${plan.id}:${plan.revision}`}
          plan={plan}
          step={step}
          onStep={setStep}
          overview={overview}
          onOpenOpportunity={onOpenOpportunity}
          onDeleted={() => setSelectedId(null)}
        />
      )}
    </div>
  );
}

function NewPlan({ offer, onCreated, onCancel }: { offer: Offer; onCreated: (plan: GtmPlan) => void; onCancel: (() => void) | null }) {
  const [name, setName] = useState(`${offer.name} — first plan`);
  const [geography, setGeography] = useState('');
  const [key] = useState(() => requestKey('plan'));
  const action = useAction();
  return (
    <section className="biz-card" aria-label="New plan">
      <h3 className="biz-section-title">New go-to-market plan</h3>
      <p className="biz-muted">
        Uses {offer.name} v{offer.currentVersion}. Research saves draft hypotheses first; nothing becomes your active
        plan until you choose a segment and an experiment.
      </p>
      <div className="biz-form-grid">
        <label className="field">
          <span className="field__label">Name</span>
          <input className="input" value={name} onChange={(e) => setName(e.target.value)} />
        </label>
        <label className="field">
          <span className="field__label">Target geography</span>
          <input className="input" placeholder="e.g. Austria and Germany" value={geography} onChange={(e) => setGeography(e.target.value)} />
        </label>
      </div>
      {action.error && (
        <p className="form-error" role="alert">
          {action.error}
        </p>
      )}
      <div className="biz-card__actions">
        {onCancel && (
          <button type="button" className="button button--ghost" onClick={onCancel}>
            Cancel
          </button>
        )}
        <button
          type="button"
          className="button button--primary"
          disabled={!name.trim() || action.busy}
          onClick={() =>
            void action.run(async () => {
              const plan = await createPlan({
                offerId: offer.id,
                offerVersion: offer.currentVersion,
                name: name.trim(),
                geography: geography.trim(),
                idempotencyKey: key,
              });
              onCreated(plan);
            })
          }
        >
          Create plan
        </button>
      </div>
    </section>
  );
}

function PlanEditor({
  plan,
  step,
  onStep: setStep,
  overview,
  onOpenOpportunity,
  onDeleted,
}: {
  plan: GtmPlan;
  step: Step;
  onStep: (step: Step) => void;
  overview: BusinessOverview;
  onOpenOpportunity: (id: string) => void;
  onDeleted: () => void;
}) {
  const [name, setName] = useState(plan.name);
  const [geography, setGeography] = useState(plan.geography);
  const [content, setContent] = useState<PlanContent>(plan.content);
  const [dirty, setDirty] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const action = useAction();
  const research = useBusinessRun();

  const update = (patch: Partial<PlanContent>) => {
    setContent((c) => ({ ...c, ...patch }));
    setDirty(true);
  };

  const save = () =>
    void action.run(() => savePlan(plan.id, { name, geography, content, expectedRevision: plan.revision }));

  const runResearch = (part: PlanPart) => void research.start((runId) => researchPlan(plan.id, part, runId));

  const researchButton = (part: PlanPart, label: string) =>
    research.running ? (
      <RunProgress status={research.status} fallback="Researching…" onStop={research.stop} />
    ) : (
      <button
        type="button"
        className="button button--secondary button--small"
        disabled={dirty}
        title={dirty ? 'Save your changes first' : undefined}
        onClick={() => runResearch(part)}
      >
        <SparkleIcon className="button__icon" />
        {label}
      </button>
    );

  return (
    <section className="biz-plan" aria-label={plan.name}>
      <div className="biz-plan__head">
        <div className="biz-form-grid biz-plan__meta">
          <label className="field">
            <span className="field__label">Plan name</span>
            <input
              className="input"
              value={name}
              onChange={(e) => {
                setName(e.target.value);
                setDirty(true);
              }}
            />
          </label>
          <label className="field">
            <span className="field__label">Geography</span>
            <input
              className="input"
              value={geography}
              onChange={(e) => {
                setGeography(e.target.value);
                setDirty(true);
              }}
            />
          </label>
        </div>
        <div className="biz-plan__actions">
          <span className="biz-muted">
            {plan.offer.name} v{plan.offer.version} · updated {formatDateTime(plan.updatedAt)}
          </span>
          <button type="button" className="button button--primary button--small" disabled={!dirty || action.busy} onClick={save}>
            Save plan
          </button>
          <button type="button" className="button button--ghost button--small" aria-label="Delete plan" onClick={() => setConfirmDelete(true)}>
            <TrashIcon className="button__icon" />
          </button>
        </div>
      </div>
      {confirmDelete && (
        <div ref={reveal} className="notice notice--danger biz-confirm" role="alertdialog" aria-label="Delete plan">
          <span>Delete this plan with its experiments? Saved opportunities and drafts stay.</span>
          <button
            type="button"
            className="button button--danger button--small"
            onClick={() =>
              void action.run(() => deletePlan(plan.id)).then((ok) => {
                if (ok) onDeleted();
              })
            }
          >
            Delete
          </button>
          <button type="button" className="button button--ghost button--small" onClick={() => setConfirmDelete(false)}>
            Cancel
          </button>
        </div>
      )}
      {(action.error || research.error) && (
        <p className="form-error" role="alert">
          {action.error ?? research.error}
        </p>
      )}
      {dirty && <p className="biz-unsaved">Unsaved changes — save the plan before running research.</p>}

      <div className="segmented biz-steps" role="tablist" aria-label="Plan steps">
        {STEPS.map((s) => (
          <button
            key={s.id}
            type="button"
            role="tab"
            aria-selected={step === s.id}
            className={step === s.id ? 'segmented__option segmented__option--active' : 'segmented__option'}
            onClick={() => setStep(s.id)}
          >
            {s.label}
          </button>
        ))}
      </div>

      {content.notes.length > 0 && (
        <ul className="biz-notes">
          {content.notes.map((n) => (
            <li key={n}>{n}</li>
          ))}
        </ul>
      )}

      {step === 'segments' && (
        <SegmentsStep
          segments={content.segments}
          geography={geography}
          onChange={(segments) => update({ segments })}
          action={researchButton('segments', content.segments.length ? 'Propose segments again' : 'Propose segments')}
        />
      )}
      {step === 'alternatives' && (
        <AlternativesStep
          plan={plan}
          content={content}
          dirty={dirty}
          onChange={update}
          action={researchButton('alternatives', content.alternatives.length ? 'Research again' : 'Research alternatives')}
        />
      )}
      {step === 'channels' && (
        <ChannelsStep
          channels={content.channels}
          onChange={(channels) => update({ channels })}
          action={researchButton('channels', content.channels.length ? 'Research again' : 'Research channels')}
        />
      )}
      {step === 'accounts' && <AccountsStep plan={plan} dirty={dirty} onOpenOpportunity={onOpenOpportunity} />}
      {step === 'drafts' && <DraftsStep plan={plan} overview={overview} />}
      {step === 'experiments' && <Experiments plan={plan} overview={overview} />}
    </section>
  );
}

function SegmentsStep({
  segments,
  geography,
  onChange,
  action,
}: {
  segments: Segment[];
  geography: string;
  onChange: (next: Segment[]) => void;
  action: ReactNode;
}) {
  const set = (index: number, patch: Partial<Segment>) =>
    onChange(segments.map((s, i) => (i === index ? { ...s, ...patch } : s)));
  return (
    <div className="biz-step">
      <div className="biz-step__intro">
        <p className="biz-muted">
          Up to three starting segments: hypotheses to test, not proof of demand. Sector membership alone is not a
          pain point. Choose the ones to use for target accounts.
        </p>
        {action}
      </div>
      {segments.map((s, index) => (
        <article key={s.id || `new-${index}`} className="biz-segment">
          <div className="biz-segment__head">
            <input
              className="input biz-segment__name"
              aria-label="Segment name"
              placeholder="Segment name"
              value={s.name}
              onChange={(e) => set(index, { name: e.target.value })}
            />
            <select
              className="input biz-segment__status"
              aria-label="Status"
              value={s.status}
              onChange={(e) => set(index, { status: e.target.value as SegmentStatus })}
            >
              {(Object.keys(SEGMENT_STATUS_LABELS) as SegmentStatus[]).map((st) => (
                <option key={st} value={st}>
                  {SEGMENT_STATUS_LABELS[st]}
                </option>
              ))}
            </select>
            <label className="biz-check">
              <input type="checkbox" checked={s.selected} onChange={(e) => set(index, { selected: e.target.checked })} />
              Use for target accounts
            </label>
            <button
              type="button"
              className="button button--ghost button--small"
              aria-label="Remove segment"
              onClick={() => onChange(segments.filter((_, i) => i !== index))}
            >
              <TrashIcon className="button__icon" />
            </button>
          </div>
          <div className="biz-form-grid">
            <label className="field">
              <span className="field__label">Organization type</span>
              <input className="input" value={s.organizationType} onChange={(e) => set(index, { organizationType: e.target.value })} />
            </label>
            <label className="field">
              <span className="field__label">Geography</span>
              <input className="input" value={s.geography} onChange={(e) => set(index, { geography: e.target.value })} />
            </label>
            <label className="field">
              <span className="field__label">Indicative size</span>
              <input className="input" value={s.sizeBand} onChange={(e) => set(index, { sizeBand: e.target.value })} />
            </label>
            <label className="field">
              <span className="field__label">Use case</span>
              <input className="input" value={s.useCase} onChange={(e) => set(index, { useCase: e.target.value })} />
            </label>
          </div>
          <label className="field">
            <span className="field__label">Pain hypothesis</span>
            <textarea className="input input--textarea" rows={2} value={s.painHypothesis} onChange={(e) => set(index, { painHypothesis: e.target.value })} />
          </label>
          <div className="biz-form-grid">
            <LinesField label="Buyer roles" value={s.buyerRoles} onChange={(v) => set(index, { buyerRoles: v })} />
            <LinesField label="Prerequisites" value={s.prerequisites} onChange={(v) => set(index, { prerequisites: v })} />
            <LinesField label="Observable signals" value={s.observableSignals} onChange={(v) => set(index, { observableSignals: v })} />
            <LinesField label="Disqualifiers" value={s.disqualifiers} onChange={(v) => set(index, { disqualifiers: v })} />
            <LinesField label="Likely objections" value={s.likelyObjections} onChange={(v) => set(index, { likelyObjections: v })} />
            <LinesField label="Unknowns" value={s.unknowns} onChange={(v) => set(index, { unknowns: v })} />
          </div>
          <LinesField label="Validation questions" value={s.validationQuestions} onChange={(v) => set(index, { validationQuestions: v })} />
          {(s.supportingEvidence.length > 0 || s.counterevidence.length > 0) && (
            <details className="biz-segment__evidence">
              <summary>
                Evidence ({s.supportingEvidence.length} supporting, {s.counterevidence.length} contrary)
              </summary>
              <SourceNotes notes={[...s.supportingEvidence, ...s.counterevidence]} />
            </details>
          )}
        </article>
      ))}
      <button type="button" className="button button--ghost button--small" onClick={() => onChange([...segments, blankSegment(geography)])}>
        <PlusIcon className="button__icon" />
        Add segment
      </button>
    </div>
  );
}

function AlternativesStep({
  plan,
  content,
  dirty,
  onChange,
  action,
}: {
  plan: GtmPlan;
  content: PlanContent;
  dirty: boolean;
  onChange: (patch: Partial<PlanContent>) => void;
  action: ReactNode;
}) {
  const saved = plan.content.segments.filter((s) => s.id);
  const [segmentId, setSegmentId] = useState(saved.find((s) => s.selected)?.id ?? saved[0]?.id ?? '');
  const [alternative, setAlternative] = useState<number | null>(null);
  const positioning = useAction();
  return (
    <div className="biz-step">
      <div className="biz-step__intro">
        <p className="biz-muted">
          Direct and adjacent products, services, building it in-house or keeping the manual process. Published claims
          stay claims; a feature missing from a page is unknown, not absent.
        </p>
        {action}
      </div>
      {content.alternatives.map((a: Alternative, index) => (
        <article key={`${a.name}-${index}`} className="biz-alt">
          <div className="biz-alt__head">
            <strong>{a.name}</strong>
            <span className="badge">{a.kind}</span>
            <button
              type="button"
              className="button button--ghost button--small"
              aria-label={`Remove ${a.name}`}
              onClick={() => onChange({ alternatives: content.alternatives.filter((_, i) => i !== index) })}
            >
              <TrashIcon className="button__icon" />
            </button>
          </div>
          <p>{a.summary}</p>
          {a.pricing && <p className="biz-muted">Pricing as published: {a.pricing}</p>}
          {a.unknowns.length > 0 && <p className="biz-muted">Unknown: {a.unknowns.join('; ')}</p>}
          <SourceNotes notes={a.evidence} />
        </article>
      ))}
      <section className="biz-card" aria-label="Positioning">
        <h3 className="biz-section-title">Positioning draft</h3>
        <p className="biz-muted">
          Built from reviewed capabilities only. Editing it never changes the offer; your untested differentiation
          claims stay labelled as untested.
        </p>
        {saved.length > 0 && (
          <div className="biz-inline-form">
            <label className="field">
              <span className="field__label">Segment</span>
              <select className="input" value={segmentId} onChange={(e) => setSegmentId(e.target.value)}>
                {saved.map((s) => (
                  <option key={s.id} value={s.id}>
                    {s.name || 'Unnamed segment'}
                  </option>
                ))}
              </select>
            </label>
            <label className="field">
              <span className="field__label">Compared with</span>
              <select
                className="input"
                value={alternative ?? ''}
                onChange={(e) => setAlternative(e.target.value === '' ? null : Number(e.target.value))}
              >
                <option value="">No specific alternative</option>
                {plan.content.alternatives.map((a, i) => (
                  <option key={`${a.name}-${i}`} value={i}>
                    {a.name}
                  </option>
                ))}
              </select>
            </label>
            <button
              type="button"
              className="button button--secondary button--small"
              disabled={!segmentId || positioning.busy || dirty}
              title={dirty ? 'Save your changes first' : undefined}
              onClick={() =>
                void positioning.run(async () => {
                  const text = await draftPositioning(plan.id, segmentId, alternative);
                  onChange({ positioning: text });
                })
              }
            >
              Draft positioning
            </button>
          </div>
        )}
        {positioning.error && (
          <p className="form-error" role="alert">
            {positioning.error}
          </p>
        )}
        <label className="field">
          <span className="field__label">Positioning</span>
          <textarea
            className="input input--textarea"
            rows={4}
            value={content.positioning}
            placeholder="For [segment] dealing with [problem], [offer] provides [reviewed capability], with [supported distinction] compared with [alternative]."
            onChange={(e) => onChange({ positioning: e.target.value })}
          />
        </label>
        <LinesField
          label="Untested differentiation claims"
          value={content.untestedClaims}
          onChange={(untestedClaims) => onChange({ untestedClaims })}
        />
      </section>
    </div>
  );
}

function ChannelsStep({
  channels,
  onChange,
  action,
}: {
  channels: ChannelPlan[];
  onChange: (next: ChannelPlan[]) => void;
  action: ReactNode;
}) {
  const set = (index: number, patch: Partial<ChannelPlan>) =>
    onChange(channels.map((c, i) => (i === index ? { ...c, ...patch } : c)));
  const text = (index: number, key: 'audience' | 'why' | 'entryPoint' | 'rules' | 'effort' | 'costs' | 'test', label: string) => (
    <label className="field">
      <span className="field__label">{label}</span>
      <input className="input" value={channels[index]?.[key] ?? ''} onChange={(e) => set(index, { [key]: e.target.value })} />
    </label>
  );
  return (
    <div className="biz-step">
      <div className="biz-step__intro">
        <p className="biz-muted">
          Where this offer could reach buyers. ReMa plans and links; it does not buy ads, join groups, publish posts,
          create listings or send messages. Community and marketplace rules are checked, never assumed.
        </p>
        {action}
      </div>
      {channels.map((c, index) => (
        <article key={index} className={c.available ? 'biz-channel' : 'biz-channel is-unavailable'}>
          <div className="biz-segment__head">
            <input
              className="input biz-segment__name"
              aria-label="Channel name"
              placeholder="Channel"
              value={c.name}
              onChange={(e) => set(index, { name: e.target.value })}
            />
            <label className="biz-check">
              <input type="checkbox" checked={!c.available} onChange={(e) => set(index, { available: !e.target.checked })} />
              Mark unavailable
            </label>
            <button
              type="button"
              className="button button--ghost button--small"
              aria-label="Remove channel"
              onClick={() => onChange(channels.filter((_, i) => i !== index))}
            >
              <TrashIcon className="button__icon" />
            </button>
          </div>
          <div className="biz-form-grid">
            {text(index, 'audience', 'Audience')}
            {text(index, 'why', 'Why it may fit')}
            {text(index, 'entryPoint', 'Entry point')}
            {text(index, 'rules', 'Access or promotion rules')}
            {text(index, 'effort', 'Effort')}
            {text(index, 'costs', 'Known costs (with source and date)')}
          </div>
          {text(index, 'test', 'A small test')}
          <LinesField label="Unknowns" value={c.unknowns} onChange={(unknowns) => set(index, { unknowns })} />
          <SourceNotes notes={c.evidence} />
        </article>
      ))}
      <button type="button" className="button button--ghost button--small" onClick={() => onChange([...channels, blankChannel()])}>
        <PlusIcon className="button__icon" />
        Add channel
      </button>
    </div>
  );
}

/** Target accounts reuse Find Clients' assessments (B21), not a second scorer. */
function AccountsStep({ plan, dirty, onOpenOpportunity }: { plan: GtmPlan; dirty: boolean; onOpenOpportunity: (id: string) => void }) {
  const segments = plan.content.segments.filter((s) => s.id);
  const ordered = [...segments.filter((s) => s.selected), ...segments.filter((s) => !s.selected)];
  const [segmentId, setSegmentId] = useState(ordered[0]?.id ?? '');
  const [saved, setSaved] = useState<Record<string, string>>({});
  const run = useBusinessRun();
  const action = useAction();
  const accounts = plan.content.targetAccounts.filter((a) => !segmentId || a.segmentId === segmentId || a.segmentId === null);
  const segment = segments.find((s) => s.id === segmentId) ?? null;
  return (
    <div className="biz-step">
      <div className="biz-step__intro">
        <p className="biz-muted">
          Companies matching a segment, with the same evidence-based fit as Find Clients. A suggested angle is a
          question to validate, not a known pain point.
        </p>
        {segments.length === 0 ? (
          <span className="biz-muted">Save a segment first.</span>
        ) : (
          <div className="biz-inline-form">
            <select className="input" aria-label="Segment" value={segmentId} onChange={(e) => setSegmentId(e.target.value)}>
              {ordered.map((s) => (
                <option key={s.id} value={s.id}>
                  {s.name || 'Unnamed segment'}
                  {s.selected ? ' (selected)' : ''}
                </option>
              ))}
            </select>
            {run.running ? (
              <RunProgress status={run.status} fallback="Finding companies…" onStop={run.stop} />
            ) : (
              <button
                type="button"
                className="button button--secondary button--small"
                disabled={!segmentId || dirty}
                title={dirty ? 'Save your changes first' : undefined}
                onClick={() => void run.start((runId) => findTargetAccounts(plan.id, segmentId, runId))}
              >
                Find target accounts
              </button>
            )}
          </div>
        )}
      </div>
      {(run.error || action.error) && (
        <p className="form-error" role="alert">
          {run.error ?? action.error}
        </p>
      )}
      {accounts.length > 0 && (
        <div className="table-wrap">
          <table className="data-table biz-table">
            <thead>
              <tr>
                <th scope="col">Company / link</th>
                <th scope="col">Why it fits</th>
                <th scope="col">Buyer role / contact</th>
                <th scope="col">Observed trigger</th>
                <th scope="col">Suggested angle / validation question</th>
                <th scope="col">Fit / evidence</th>
                <th scope="col">Pipeline</th>
              </tr>
            </thead>
            <tbody>
              {accounts.map((a) => {
                const opportunityId = a.opportunityId ?? saved[a.companyKey] ?? null;
                return (
                  <tr key={a.companyKey}>
                    <td className="biz-table__name">
                      {a.companyName}
                      {a.suppressed && <span className="badge badge--danger">Do not contact</span>}
                      {a.link && (
                        <span className="biz-person__title">
                          <Links links={[{ label: a.contact ? 'Profile' : 'Contact page', url: a.link }]} />
                        </span>
                      )}
                    </td>
                    <td className="biz-table__why">{a.why}</td>
                    <td>
                      {a.contact ?? a.buyerRole}
                      {a.contact && <span className="biz-muted biz-person__title">{a.buyerRole}</span>}
                    </td>
                    <td className="biz-table__signal">{a.trigger ?? <span className="biz-muted">None found</span>}</td>
                    <td className="biz-table__why">
                      {a.angle}
                      <span className="biz-muted biz-person__title">Ask: {a.question}</span>
                    </td>
                    <td className="biz-table__fit">
                      <FitCell score={a.fit} coverage={a.coverage} shown={a.fit !== null} />
                    </td>
                    <td>
                      {opportunityId ? (
                        <button type="button" className="link-button" onClick={() => onOpenOpportunity(opportunityId)}>
                          Open
                        </button>
                      ) : a.runId ? (
                        <button
                          type="button"
                          className="button button--secondary button--small"
                          disabled={action.busy}
                          onClick={() =>
                            void action.run(async () => {
                              const result = await saveProspect(a.runId ?? '', a.companyKey, segment?.useCase || null);
                              setSaved((s) => ({ ...s, [a.companyKey]: result.opportunity.id }));
                            })
                          }
                        >
                          Save
                        </button>
                      ) : (
                        <span className="biz-muted">—</span>
                      )}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
      {accounts.length === 0 && !run.running && segments.length > 0 && (
        <p className="biz-empty">No target accounts for this segment yet.</p>
      )}
    </div>
  );
}

function DraftsStep({ plan, overview }: { plan: GtmPlan; overview: BusinessOverview }) {
  const pipeline = dataOr(usePipeline().state, null);
  const opportunities = (pipeline?.opportunities ?? []).filter(
    (o) => o.offer?.offerId === plan.offer.offerId && !o.archived && !o.doNotContact,
  );
  const [opportunityId, setOpportunityId] = useState('');
  const opportunity = opportunities.find((o) => o.id === opportunityId) ?? null;
  const experiments = overview.experiments.filter((e) => e.planId === plan.id && e.status !== 'cancelled');
  const experimentIds = new Set(experiments.map((e) => e.id));
  const drafts = overview.drafts.filter(
    (d) => d.planId === plan.id || (d.experimentId !== null && experimentIds.has(d.experimentId)),
  );
  const segment = plan.content.segments.find((s) => s.selected) ?? plan.content.segments[0] ?? null;
  return (
    <div className="biz-step">
      <p className="biz-muted">
        Short, professional drafts from the reviewed offer and saved evidence. Every draft stays local: copy it and
        send it yourself, then record the contact in the Pipeline.
      </p>
      <section className="biz-card" aria-label="New draft">
        <label className="field">
          <span className="field__label">For</span>
          <select className="input" value={opportunityId} onChange={(e) => setOpportunityId(e.target.value)}>
            <option value="">A buyer role in this plan (no specific company)</option>
            {opportunities.map((o) => (
              <option key={o.id} value={o.id}>
                {o.companyName ?? o.name} — {o.name}
              </option>
            ))}
          </select>
        </label>
        <NewDraftForm
          key={opportunityId || 'plan'}
          opportunityId={opportunity?.id ?? null}
          planId={plan.id}
          contacts={opportunity?.contacts ?? []}
          defaultRole={opportunity?.contacts[0]?.role ?? segment?.buyerRoles[0] ?? ''}
          experiments={experiments}
        />
      </section>
      {drafts.map((d) => (
        <DraftCard key={`${d.id}-${d.revision}`} draft={d} />
      ))}
    </div>
  );
}
