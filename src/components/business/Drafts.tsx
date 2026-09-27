import { useState } from 'react';

import { useAction } from '../../hooks/useAction';
import { useBusinessRun } from '../../hooks/useBusiness';
import { formatDateTime } from '../../lib/format';
import {
  createDraft,
  deleteDraft,
  requestKey,
  updateDraft,
  type ContactRef,
  type Draft,
  type Experiment,
} from '../../services/businessService';
import { CopyIcon, TrashIcon } from '../icons';
import { RunProgress, SourceNotes } from './common';
import { CHANNELS, copyText } from './helpers';

/**
 * A local draft (B21): previewable, editable and copyable. ReMa has no
 * sending action; copying never records a contact or changes a stage.
 */
export function DraftCard({ draft }: { draft: Draft }) {
  const [subject, setSubject] = useState(draft.subject ?? '');
  const [body, setBody] = useState(draft.body);
  const [revision, setRevision] = useState(draft.revision);
  const [note, setNote] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const action = useAction();
  const dirty = subject !== (draft.subject ?? '') || body !== draft.body;
  return (
    <article className="biz-draft" aria-label={`Draft to ${draft.recipient}`}>
      <header className="biz-draft__head">
        <span className="biz-draft__to">
          To {draft.recipient} · {draft.channel}
        </span>
        <span className="badge">Local draft — not sent</span>
        {draft.variant && <span className="badge badge--brand">Variant {draft.variant}</span>}
        <span className="biz-muted biz-draft__time">
          {draft.offer ? `${draft.offer.name} v${draft.offer.version} · ` : ''}
          {formatDateTime(draft.updatedAt)}
        </span>
      </header>
      <input
        className="input biz-draft__subject"
        aria-label="Subject"
        placeholder="Subject (optional)"
        value={subject}
        onChange={(e) => {
          setSubject(e.target.value);
          setNote(null);
        }}
      />
      <textarea
        className="input input--textarea biz-draft__body"
        aria-label="Message"
        rows={7}
        value={body}
        onChange={(e) => {
          setBody(e.target.value);
          setNote(null);
        }}
      />
      {draft.evidence.length > 0 && (
        <details className="biz-draft__evidence">
          <summary>Evidence behind this draft</summary>
          <SourceNotes notes={draft.evidence} />
        </details>
      )}
      <div className="biz-draft__actions">
        <button
          type="button"
          className="button button--secondary button--small"
          onClick={() =>
            void copyText(subject ? `${subject}\n\n${body}` : body).then((ok) =>
              setNote(ok ? 'Copied. Copying is not contact: record it in the Pipeline once you actually send it.' : 'Could not copy.'),
            )
          }
        >
          <CopyIcon className="button__icon" />
          Copy draft
        </button>
        <button
          type="button"
          className="button button--ghost button--small"
          disabled={!dirty || action.busy}
          onClick={() =>
            void action.run(async () => {
              const next = await updateDraft(draft.id, subject.trim() || null, body, revision);
              setRevision(next.revision);
              setNote('Saved.');
            })
          }
        >
          Save changes
        </button>
        <button
          type="button"
          className="button button--ghost button--small"
          aria-label="Delete draft"
          onClick={() => setConfirmDelete(true)}
        >
          <TrashIcon className="button__icon" />
        </button>
        {note && (
          <span className="biz-muted" role="status">
            {note}
          </span>
        )}
      </div>
      {confirmDelete && (
        <div className="notice notice--danger biz-confirm" role="alertdialog" aria-label="Delete draft">
          <span>Delete this draft?</span>
          <button
            type="button"
            className="button button--danger button--small"
            onClick={() => void action.run(() => deleteDraft(draft.id))}
          >
            Delete
          </button>
          <button type="button" className="button button--ghost button--small" onClick={() => setConfirmDelete(false)}>
            Cancel
          </button>
        </div>
      )}
      {action.error && (
        <p className="form-error" role="alert">
          {action.error}
        </p>
      )}
    </article>
  );
}

/** Asks for a new draft: a contact or a buyer role, a channel, optionally a variant. */
export function NewDraftForm({
  opportunityId,
  planId,
  contacts,
  defaultRole,
  experiments,
}: {
  opportunityId: string | null;
  planId: string | null;
  contacts: ContactRef[];
  defaultRole: string;
  experiments: Experiment[];
}) {
  const [channel, setChannel] = useState(CHANNELS[0] ?? 'Email');
  const [contactId, setContactId] = useState<string>('');
  const [role, setRole] = useState(defaultRole);
  const [experimentId, setExperimentId] = useState('');
  const [variant, setVariant] = useState('');
  const [key, setKey] = useState(() => requestKey('draft'));
  const [created, setCreated] = useState(false);
  const run = useBusinessRun();
  const experiment = experiments.find((e) => e.id === experimentId) ?? null;
  const ready = contactId !== '' || role.trim() !== '';

  const submit = () =>
    void run
      .start((runId) =>
        createDraft(
          {
            opportunityId,
            planId,
            experimentId: experimentId || null,
            variant: experiment && variant ? variant : null,
            contactId: contactId || null,
            role: contactId ? null : role.trim() || null,
            channel,
            idempotencyKey: key,
          },
          runId,
        ),
      )
      .then((draft) => {
        if (draft) {
          setCreated(true);
          setKey(requestKey('draft'));
        }
      });

  return (
    <div className="biz-new-draft">
      <div className="biz-form-grid">
        {contacts.length > 0 && (
          <label className="field">
            <span className="field__label">To</span>
            <select className="input" value={contactId} onChange={(e) => setContactId(e.target.value)}>
              <option value="">A buyer role (no named person)</option>
              {contacts.map((c) => (
                <option key={c.id} value={c.id}>
                  {c.name ? `${c.name} — ${c.title ?? c.role}` : c.role}
                </option>
              ))}
            </select>
          </label>
        )}
        {!contactId && (
          <label className="field">
            <span className="field__label">Buyer role</span>
            <input className="input" placeholder="e.g. Head of Operations" value={role} onChange={(e) => setRole(e.target.value)} />
          </label>
        )}
        <label className="field">
          <span className="field__label">Channel</span>
          <select className="input" value={channel} onChange={(e) => setChannel(e.target.value)}>
            {CHANNELS.map((c) => (
              <option key={c} value={c}>
                {c}
              </option>
            ))}
          </select>
        </label>
        {experiments.length > 0 && (
          <label className="field">
            <span className="field__label">Experiment (optional)</span>
            <select
              className="input"
              value={experimentId}
              onChange={(e) => {
                setExperimentId(e.target.value);
                setVariant('');
              }}
            >
              <option value="">None</option>
              {experiments.map((e) => (
                <option key={e.id} value={e.id}>
                  {e.content.hypothesis.slice(0, 60) || 'Experiment'} (v{e.version})
                </option>
              ))}
            </select>
          </label>
        )}
        {experiment && (
          <label className="field">
            <span className="field__label">Message variant</span>
            <select className="input" value={variant} onChange={(e) => setVariant(e.target.value)}>
              <option value="">None</option>
              {experiment.content.variants.map((v) => (
                <option key={v.id} value={v.id}>
                  {v.label}
                </option>
              ))}
            </select>
          </label>
        )}
      </div>
      <p className="form__hint">
        Drafts use the reviewed offer version and saved evidence. A need that is not confirmed is asked as a question,
        never stated as fact. ReMa does not send anything.
      </p>
      {run.running && <RunProgress status={run.status} fallback="Drafting…" onStop={run.stop} />}
      {run.error && (
        <p className="form-error" role="alert">
          {run.error}
        </p>
      )}
      {created && !run.running && (
        <p className="biz-saved" role="status">
          Draft saved below.
        </p>
      )}
      <div className="biz-card__actions">
        <button type="button" className="button button--primary button--small" disabled={!ready || run.running} onClick={submit}>
          Write draft
        </button>
      </div>
    </div>
  );
}
