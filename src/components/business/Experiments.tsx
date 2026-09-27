import { useCallback, useState } from 'react';

import { useAction } from '../../hooks/useAction';
import { dataOr, useAsyncData } from '../../hooks/useAsyncData';
import { useBackendEvent } from '../../hooks/useBackendEvent';
import { usePipeline } from '../../hooks/useBusiness';
import { formatDate, formatDateTime } from '../../lib/format';
import {
  amendExperiment,
  createExperiment,
  deleteExperiment,
  getExperimentMetrics,
  requestKey,
  setExperimentStatus,
  updateExperiment,
  type Assignment,
  type BusinessOverview,
  type CohortEntry,
  type Experiment,
  type ExperimentContent,
  type ExperimentStatus,
  type GtmPlan,
  type VariantMetrics,
} from '../../services/businessService';
import { backendEvents } from '../../services/events';
import { PlusIcon, TrashIcon } from '../icons';
import { fromDateInput, rateText, reveal, toDateInput } from './helpers';
import { ASSIGNMENT_LABELS, EXPERIMENT_STATUS_LABELS } from './labels';

const METRICS: Record<string, string> = {
  reply_rate: 'Reply rate',
  positive_reply_rate: 'Positive reply rate',
  meeting_rate: 'Meeting rate',
  proposal_rate: 'Proposal rate',
  win_rate: 'Win rate',
};

const accounts = (n: number) => `${n} ${n === 1 ? 'account' : 'accounts'}`;

function newContent(): ExperimentContent {
  return {
    hypothesis: '',
    channel: 'Email',
    variants: [
      { id: 'v1', label: 'A', message: '' },
      { id: 'v2', label: 'B', message: '' },
    ],
    assignment: 'random_by_account',
    cohort: [],
    primaryMetric: 'reply_rate',
    successThreshold: null,
    plannedStart: null,
    plannedEnd: null,
    observationDays: 21,
    sampleTarget: 30,
    budget: null,
    effortBudget: null,
    stopConditions: [],
    exclusions: [],
    outcomeSummary: null,
    limitations: [],
  };
}

/** Accounts that can join a cohort: the plan's target accounts and saved opportunities for this offer. */
function useCandidates(plan: GtmPlan): CohortEntry[] {
  const pipeline = dataOr(usePipeline().state, null);
  const out = new Map<string, CohortEntry>();
  for (const o of pipeline?.opportunities ?? []) {
    if (o.offer?.offerId !== plan.offer.offerId || o.archived) continue;
    const key = o.companyKey ?? `opportunity:${o.id}`;
    out.set(key, { accountKey: key, accountName: o.companyName ?? o.name, opportunityId: o.id, variant: null });
  }
  for (const a of plan.content.targetAccounts) {
    if (!out.has(a.companyKey)) {
      out.set(a.companyKey, { accountKey: a.companyKey, accountName: a.companyName, opportunityId: a.opportunityId, variant: null });
    }
  }
  return [...out.values()].sort((a, b) => a.accountName.localeCompare(b.accountName));
}

/**
 * Experiments (B22–B23): a hypothesis and a measurable plan. Starting one
 * begins recordkeeping only — it never sends, schedules or spends.
 */
export function Experiments({ plan, overview }: { plan: GtmPlan; overview: BusinessOverview }) {
  const experiments = overview.experiments
    .filter((e) => e.planId === plan.id)
    .sort((a, b) => b.createdAt - a.createdAt);
  const [creating, setCreating] = useState(false);
  return (
    <div className="biz-step">
      <div className="biz-step__intro">
        <p className="biz-muted">
          Counts come from activity you record in the Pipeline, per account, within the cohort and window. A proposed
          pilot (for example 30 accounts and three variants) is not a statistically sufficient sample.
        </p>
        {!creating && (
          <button type="button" className="button button--secondary button--small" onClick={() => setCreating(true)}>
            <PlusIcon className="button__icon" />
            New experiment
          </button>
        )}
      </div>
      {creating && (
        <ExperimentEditor plan={plan} experiment={null} onDone={() => setCreating(false)} />
      )}
      {experiments.map((e) => (
        <ExperimentCard key={`${e.id}-${e.revision}`} experiment={e} plan={plan} />
      ))}
    </div>
  );
}

function ExperimentEditor({
  plan,
  experiment,
  onDone,
}: {
  plan: GtmPlan;
  experiment: Experiment | null;
  onDone: () => void;
}) {
  const [content, setContent] = useState<ExperimentContent>(experiment?.content ?? newContent());
  const [segmentId, setSegmentId] = useState(
    experiment?.segmentId ?? plan.content.segments.find((s) => s.selected)?.id ?? plan.content.segments[0]?.id ?? '',
  );
  const [key] = useState(() => requestKey('exp'));
  const candidates = useCandidates(plan);
  const action = useAction();
  const frozen = experiment?.frozenAt != null;
  const set = (patch: Partial<ExperimentContent>) => setContent((c) => ({ ...c, ...patch }));
  const inCohort = new Set(content.cohort.map((c) => c.accountKey));
  const [stopText, setStopText] = useState(content.stopConditions.join('\n'));
  const [exclusionText, setExclusionText] = useState(content.exclusions.join('\n'));
  const [limitText, setLimitText] = useState(content.limitations.join('\n'));
  const lines = (t: string) =>
    t
      .split('\n')
      .map((l) => l.trim())
      .filter(Boolean);

  const submit = () =>
    void action.run(async () => {
      if (experiment) await updateExperiment(experiment.id, content, experiment.revision);
      else await createExperiment({ planId: plan.id, segmentId: segmentId || null, content, idempotencyKey: key });
      onDone();
    });

  return (
    <section className="biz-card biz-experiment-editor" aria-label={experiment ? 'Edit experiment' : 'New experiment'}>
      {frozen && (
        <p className="notice notice--warning">
          Frozen since {formatDateTime(experiment?.frozenAt ?? 0)}: only the outcome, limitations and stop conditions
          can change. Create an amendment to change the plan.
        </p>
      )}
      <fieldset disabled={frozen} className="biz-fieldset">
        <label className="field">
          <span className="field__label">Hypothesis</span>
          <textarea
            className="input input--textarea"
            rows={2}
            placeholder="Operations leads at mid-sized manufacturers reply more to a question about downtime than to a feature list."
            value={content.hypothesis}
            onChange={(e) => set({ hypothesis: e.target.value })}
          />
        </label>
        <div className="biz-form-grid">
          {plan.content.segments.length > 0 && !experiment && (
            <label className="field">
              <span className="field__label">Segment</span>
              <select className="input" value={segmentId} onChange={(e) => setSegmentId(e.target.value)}>
                <option value="">None</option>
                {plan.content.segments
                  .filter((s) => s.id)
                  .map((s) => (
                    <option key={s.id} value={s.id}>
                      {s.name || 'Unnamed segment'}
                    </option>
                  ))}
              </select>
            </label>
          )}
          <label className="field">
            <span className="field__label">Channel</span>
            <input className="input" value={content.channel} onChange={(e) => set({ channel: e.target.value })} />
          </label>
          <label className="field">
            <span className="field__label">Primary metric</span>
            <select className="input" value={content.primaryMetric} onChange={(e) => set({ primaryMetric: e.target.value })}>
              {Object.entries(METRICS).map(([id, label]) => (
                <option key={id} value={id}>
                  {label}
                </option>
              ))}
            </select>
          </label>
          <label className="field">
            <span className="field__label">Success threshold</span>
            <input
              className="input"
              placeholder="e.g. at least 3 / 30 replies"
              value={content.successThreshold ?? ''}
              onChange={(e) => set({ successThreshold: e.target.value || null })}
            />
          </label>
          <label className="field">
            <span className="field__label">Planned start</span>
            <input
              className="input"
              type="date"
              value={toDateInput(content.plannedStart)}
              onChange={(e) => set({ plannedStart: fromDateInput(e.target.value) })}
            />
          </label>
          <label className="field">
            <span className="field__label">Planned end</span>
            <input
              className="input"
              type="date"
              value={toDateInput(content.plannedEnd)}
              onChange={(e) => set({ plannedEnd: fromDateInput(e.target.value) })}
            />
          </label>
          <label className="field">
            <span className="field__label">Observation window (days after the end)</span>
            <input
              className="input input--number"
              type="number"
              min={1}
              max={180}
              value={content.observationDays}
              onChange={(e) => set({ observationDays: Number(e.target.value) || 21 })}
            />
          </label>
          <label className="field">
            <span className="field__label">Proposed pilot size (accounts)</span>
            <input
              className="input input--number"
              type="number"
              min={1}
              value={content.sampleTarget ?? ''}
              onChange={(e) => set({ sampleTarget: e.target.value ? Number(e.target.value) : null })}
            />
          </label>
          <label className="field">
            <span className="field__label">Budget (optional)</span>
            <input className="input" value={content.budget ?? ''} onChange={(e) => set({ budget: e.target.value || null })} />
          </label>
          <label className="field">
            <span className="field__label">Effort budget (optional)</span>
            <input
              className="input"
              placeholder="e.g. 3 hours a week"
              value={content.effortBudget ?? ''}
              onChange={(e) => set({ effortBudget: e.target.value || null })}
            />
          </label>
        </div>

        <div className="biz-field">
          <span className="field__label">Message variants</span>
          {content.variants.map((v, index) => (
            <div key={v.id} className="biz-variant">
              <input
                className="input biz-variant__label"
                aria-label={`Variant ${index + 1} label`}
                value={v.label}
                onChange={(e) =>
                  set({ variants: content.variants.map((x, i) => (i === index ? { ...x, label: e.target.value } : x)) })
                }
              />
              <textarea
                className="input input--textarea biz-variant__message"
                aria-label={`Variant ${v.label} message angle`}
                rows={2}
                placeholder="The message angle for this variant"
                value={v.message}
                onChange={(e) =>
                  set({ variants: content.variants.map((x, i) => (i === index ? { ...x, message: e.target.value } : x)) })
                }
              />
              {content.variants.length > 1 && (
                <button
                  type="button"
                  className="button button--ghost button--small"
                  aria-label={`Remove variant ${v.label}`}
                  onClick={() => set({ variants: content.variants.filter((_, i) => i !== index) })}
                >
                  <TrashIcon className="button__icon" />
                </button>
              )}
            </div>
          ))}
          {content.variants.length < 5 && (
            <button
              type="button"
              className="button button--ghost button--small biz-field__add"
              onClick={() => {
                const n = content.variants.length + 1;
                let id = `v${n}`;
                while (content.variants.some((v) => v.id === id)) id = `${id}x`;
                set({ variants: [...content.variants, { id, label: String.fromCharCode(64 + n), message: '' }] });
              }}
            >
              <PlusIcon className="button__icon" />
              Add variant
            </button>
          )}
        </div>

        <label className="field">
          <span className="field__label">Assignment</span>
          <select className="input" value={content.assignment} onChange={(e) => set({ assignment: e.target.value as Assignment })}>
            {(Object.keys(ASSIGNMENT_LABELS) as Assignment[]).map((a) => (
              <option key={a} value={a}>
                {ASSIGNMENT_LABELS[a]}
              </option>
            ))}
          </select>
        </label>

        <div className="biz-field">
          <span className="field__label">
            Account cohort ({content.cohort.length}
            {content.sampleTarget ? ` of a proposed ${content.sampleTarget}` : ''})
          </span>
          {candidates.length === 0 && (
            <span className="biz-muted">Find target accounts or save opportunities for this offer first.</span>
          )}
          <ul className="biz-cohort">
            {candidates.map((c) => {
              const entry = content.cohort.find((x) => x.accountKey === c.accountKey);
              return (
                <li key={c.accountKey}>
                  <label className="biz-check">
                    <input
                      type="checkbox"
                      checked={inCohort.has(c.accountKey)}
                      onChange={(e) =>
                        set({
                          cohort: e.target.checked
                            ? [...content.cohort, c]
                            : content.cohort.filter((x) => x.accountKey !== c.accountKey),
                        })
                      }
                    />
                    {c.accountName}
                    {!c.opportunityId && <span className="biz-muted"> · not in Pipeline</span>}
                  </label>
                  {entry && content.assignment === 'manual' && (
                    <select
                      className="input biz-cohort__variant"
                      aria-label={`Variant for ${c.accountName}`}
                      value={entry.variant ?? ''}
                      onChange={(e) =>
                        set({
                          cohort: content.cohort.map((x) =>
                            x.accountKey === c.accountKey ? { ...x, variant: e.target.value || null } : x,
                          ),
                        })
                      }
                    >
                      <option value="">Choose a variant</option>
                      {content.variants.map((v) => (
                        <option key={v.id} value={v.id}>
                          {v.label}
                        </option>
                      ))}
                    </select>
                  )}
                </li>
              );
            })}
          </ul>
        </div>

        <label className="field">
          <span className="field__label">Exclusions (one per line)</span>
          <textarea
            className="input input--textarea"
            rows={2}
            value={exclusionText}
            onChange={(e) => {
              setExclusionText(e.target.value);
              set({ exclusions: lines(e.target.value) });
            }}
          />
        </label>
      </fieldset>

      <label className="field">
        <span className="field__label">Stop conditions (one per line)</span>
        <textarea
          className="input input--textarea"
          rows={2}
          value={stopText}
          onChange={(e) => {
            setStopText(e.target.value);
            set({ stopConditions: lines(e.target.value) });
          }}
        />
      </label>
      {experiment && (
        <>
          <label className="field">
            <span className="field__label">Outcome summary</span>
            <textarea
              className="input input--textarea"
              rows={2}
              value={content.outcomeSummary ?? ''}
              onChange={(e) => set({ outcomeSummary: e.target.value || null })}
            />
          </label>
          <label className="field">
            <span className="field__label">Limitations (one per line)</span>
            <textarea
              className="input input--textarea"
              rows={2}
              value={limitText}
              onChange={(e) => {
                setLimitText(e.target.value);
                set({ limitations: lines(e.target.value) });
              }}
            />
          </label>
        </>
      )}
      {action.error && (
        <p className="form-error" role="alert">
          {action.error}
        </p>
      )}
      <div className="biz-card__actions">
        <button type="button" className="button button--ghost" onClick={onDone}>
          Cancel
        </button>
        <button type="button" className="button button--primary" disabled={action.busy || !content.hypothesis.trim()} onClick={submit}>
          {experiment ? 'Save experiment' : 'Create draft experiment'}
        </button>
      </div>
    </section>
  );
}

const TRANSITIONS: Record<ExperimentStatus, { to: ExperimentStatus; label: string; primary?: boolean }[]> = {
  draft: [
    { to: 'planned', label: 'Mark planned' },
    { to: 'running', label: 'Start recordkeeping', primary: true },
    { to: 'cancelled', label: 'Cancel experiment' },
  ],
  planned: [
    { to: 'draft', label: 'Back to draft' },
    { to: 'running', label: 'Start recordkeeping', primary: true },
    { to: 'cancelled', label: 'Cancel experiment' },
  ],
  running: [
    { to: 'paused', label: 'Pause' },
    { to: 'completed', label: 'Complete', primary: true },
    { to: 'cancelled', label: 'Cancel experiment' },
  ],
  paused: [
    { to: 'running', label: 'Resume', primary: true },
    { to: 'completed', label: 'Complete' },
    { to: 'cancelled', label: 'Cancel experiment' },
  ],
  completed: [],
  cancelled: [],
};

function ExperimentCard({ experiment: e, plan }: { experiment: Experiment; plan: GtmPlan }) {
  const [editing, setEditing] = useState(false);
  const [showMetrics, setShowMetrics] = useState(e.status !== 'draft' && e.status !== 'planned');
  const [confirm, setConfirm] = useState<ExperimentStatus | 'delete' | null>(null);
  const [amendKey] = useState(() => requestKey('amend'));
  const action = useAction();
  const segment = plan.content.segments.find((s) => s.id === e.segmentId) ?? null;
  const variantLabel = (id: string | null) => e.content.variants.find((v) => v.id === id)?.label ?? id ?? '—';

  if (editing) return <ExperimentEditor plan={plan} experiment={e} onDone={() => setEditing(false)} />;

  return (
    <article className="biz-experiment" aria-label={e.content.hypothesis || 'Experiment'}>
      <header className="biz-experiment__head">
        <span className="badge badge--brand">{EXPERIMENT_STATUS_LABELS[e.status]}</span>
        <span className="biz-muted">
          v{e.version} · {e.offer.name} v{e.offer.version}
          {segment && ` · ${segment.name}`} · {ASSIGNMENT_LABELS[e.content.assignment]}
        </span>
      </header>
      <p className="biz-experiment__hypothesis">{e.content.hypothesis}</p>
      <p className="biz-muted">
        {e.content.channel} · {e.content.variants.map((v) => v.label).join(', ')} · {accounts(e.content.cohort.length)} ·{' '}
        {METRICS[e.content.primaryMetric] ?? e.content.primaryMetric}
        {e.content.successThreshold && ` · success: ${e.content.successThreshold}`}
        {e.frozenAt !== null && ` · frozen ${formatDate(e.frozenAt)}`}
      </p>
      {e.frozenAt !== null && e.content.cohort.length > 0 && (
        <details className="biz-experiment__cohort">
          <summary>Frozen assignment</summary>
          <ul>
            {e.content.cohort.map((c) => (
              <li key={c.accountKey}>
                {c.accountName} → {variantLabel(c.variant)}
              </li>
            ))}
          </ul>
        </details>
      )}
      {e.content.outcomeSummary && <p>Outcome: {e.content.outcomeSummary}</p>}
      <div className="biz-experiment__actions">
        {TRANSITIONS[e.status].map((t) => (
          <button
            key={t.to}
            type="button"
            className={t.primary ? 'button button--primary button--small' : 'button button--ghost button--small'}
            disabled={action.busy}
            onClick={() => setConfirm(t.to)}
          >
            {t.label}
          </button>
        ))}
        <button type="button" className="button button--ghost button--small" onClick={() => setEditing(true)}>
          Edit
        </button>
        {e.frozenAt !== null && (
          <button
            type="button"
            className="button button--ghost button--small"
            disabled={action.busy}
            onClick={() => void action.run(() => amendExperiment(e.id, amendKey))}
          >
            Amend as new version
          </button>
        )}
        <button type="button" className="button button--ghost button--small" onClick={() => setShowMetrics(!showMetrics)}>
          {showMetrics ? 'Hide results' : 'Show results'}
        </button>
        <button type="button" className="button button--ghost button--small" aria-label="Delete experiment" onClick={() => setConfirm('delete')}>
          <TrashIcon className="button__icon" />
        </button>
      </div>
      {confirm && (
        <div
          ref={reveal}
          className={confirm === 'delete' || confirm === 'cancelled' ? 'notice notice--danger biz-confirm' : 'notice biz-confirm'}
          role="alertdialog"
          aria-label="Confirm"
        >
          <span>
            {confirm === 'delete'
              ? 'Delete this experiment? Recorded activity stays on the opportunities.'
              : confirm === 'running' && e.frozenAt === null
                ? 'Starting freezes the variants and the account assignment. It begins recordkeeping only: nothing is sent, scheduled or spent.'
                : `Move to ${EXPERIMENT_STATUS_LABELS[confirm]}?`}
          </span>
          <button
            type="button"
            className={confirm === 'delete' ? 'button button--danger button--small' : 'button button--primary button--small'}
            disabled={action.busy}
            onClick={() =>
              void action
                .run(() => (confirm === 'delete' ? deleteExperiment(e.id) : setExperimentStatus(e.id, confirm, e.revision)))
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
      {action.error && (
        <p className="form-error" role="alert">
          {action.error}
        </p>
      )}
      {showMetrics && <Metrics experiment={e} />}
    </article>
  );
}

/** Deterministic, account-level results from recorded activity (B23). */
function Metrics({ experiment }: { experiment: Experiment }) {
  const load = useCallback(() => getExperimentMetrics(experiment.id), [experiment.id]);
  const metrics = useAsyncData(load);
  useBackendEvent(backendEvents.businessChanged, metrics.refresh);
  const m = dataOr(metrics.state, null);
  if (metrics.state.status === 'error') {
    return (
      <p className="form-error" role="alert">
        {metrics.state.error.message}
      </p>
    );
  }
  if (!m) return null;
  const label = (row: VariantMetrics) =>
    row.variant === null ? 'All accounts' : (experiment.content.variants.find((v) => v.id === row.variant)?.label ?? row.variant);
  const rows = [m.overall, ...m.variants];
  return (
    <div className="biz-metrics">
      <p className="biz-muted">
        {m.windowStart !== null
          ? `Window ${formatDate(m.windowStart)} – ${m.windowEnd !== null ? formatDate(m.windowEnd) : 'open'}`
          : 'The window opens when recordkeeping starts.'}{' '}
        · user-reported activity · computed {formatDateTime(m.computedAt)}
      </p>
      <div className="table-wrap">
        <table className="data-table biz-metrics__table">
          <thead>
            <tr>
              <th scope="col">Variant</th>
              <th scope="col">Cohort</th>
              <th scope="col">Contacted</th>
              <th scope="col">Reply rate</th>
              <th scope="col">Positive replies</th>
              <th scope="col">Meetings held</th>
              <th scope="col">Proposals sent</th>
              <th scope="col">Won</th>
              <th scope="col">People contacted</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((row, i) => (
              <tr key={row.variant ?? `all-${i}`} className={i === 0 ? 'biz-metrics__overall' : undefined}>
                <td>{label(row)}</td>
                <td className="biz-num">{accounts(row.accountsInCohort)}</td>
                <td className="biz-num">{accounts(row.accountsContacted)}</td>
                <td className="biz-num">{rateText(row.replyRate)}</td>
                <td className="biz-num">{rateText(row.positiveReplyRate)}</td>
                <td className="biz-num">{rateText(row.meetingRate)}</td>
                <td className="biz-num">{rateText(row.proposalRate)}</td>
                <td className="biz-num">{rateText(row.winRate)}</td>
                <td className="biz-num">
                  {row.peopleContacted} {row.peopleContacted === 1 ? 'person' : 'people'}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      {m.limitations.length > 0 && (
        <ul className="biz-notes" aria-label="Limitations">
          {m.limitations.map((l) => (
            <li key={l}>{l}</li>
          ))}
        </ul>
      )}
      {m.unattributed.length > 0 && (
        <details className="biz-metrics__unattributed">
          <summary>Not attributed ({m.unattributed.length})</summary>
          <ul>
            {m.unattributed.map((u) => (
              <li key={u}>{u}</li>
            ))}
          </ul>
        </details>
      )}
    </div>
  );
}
