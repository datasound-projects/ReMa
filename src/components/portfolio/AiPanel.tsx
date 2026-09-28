import { useState } from 'react';

import { dataOr } from '../../hooks/useAsyncData';
import { useProfile } from '../../hooks/useProfile';
import { getField, replaceSelection, setField } from '../../lib/portfolio/fields';
import { stripMarkup } from '../../lib/portfolio/layout';
import { toApiError } from '../../services/ipc';
import { portfolioAiAssist, type AiAction, type AiProposal, type AiScope, type PortfolioInput } from '../../services/portfolioService';
import { SparkleIcon } from '../icons';
import type { CanvasSelection, TextSelection } from './canvas/DocumentCanvas';

interface AiPanelProps {
  draft: PortfolioInput;
  edit: (update: (d: PortfolioInput) => PortfolioInput, group?: string) => void;
  textSelection: TextSelection | null;
  canvasSelection: CanvasSelection;
  /** A selection handed over from the canvas toolbar (“AI” button); a new object each time. */
  requested: TextSelection | null;
}

const ACTIONS: { action: AiAction; label: string; hint: string }[] = [
  { action: 'improve', label: 'Improve wording', hint: 'Clearer and more professional, no new facts' },
  { action: 'shorten', label: 'Shorten', hint: 'Keep every fact that matters' },
  { action: 'expand', label: 'Expand', hint: 'A little more detail from what is there' },
  { action: 'achievements', label: 'Achievement bullets', hint: 'Strong verbs, results first' },
  { action: 'tailor', label: 'Tailor to a job', hint: 'Emphasise what the posting asks for' },
  { action: 'translate', label: 'Translate', hint: 'Into another language' },
  { action: 'tone', label: 'Change tone', hint: 'Confident, formal, friendly…' },
  { action: 'suggest_missing', label: 'Suggest missing information', hint: 'What a strong application still needs' },
  { action: 'populate', label: 'Fill in from…', hint: 'Your Profile or an older CV' },
  { action: 'custom', label: 'Free instruction', hint: 'Tell the assistant what to do' },
];

interface Pending {
  proposal: AiProposal;
  request: { action: AiAction; scope: AiScope; selection: TextSelection | null; sectionId: string };
}

/**
 * The AI assistant: proposals for the selection, a section or the whole
 * document, reviewed with Accept, Reject and Undo. Uses the model chosen
 * in Settings; nothing is applied without the user.
 */
export function AiPanel({ draft, edit, textSelection, canvasSelection, requested }: AiPanelProps) {
  const isLetter = (draft.kind ?? 'cv') === 'cover_letter';
  const profile = useProfile();
  const documents = (dataOr(profile.state, null)?.documents ?? []).filter((d) => d.hasText);
  const [action, setAction] = useState<AiAction>('improve');
  const [scope, setScope] = useState<AiScope>('document');
  const [instruction, setInstruction] = useState('');
  const [target, setTarget] = useState('');
  const [jobText, setJobText] = useState('');
  const [source, setSource] = useState<'profile' | 'document'>('profile');
  const [sourceDocumentId, setSourceDocumentId] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState<Pending | null>(null);
  const [pinnedSelection, setPinnedSelection] = useState<TextSelection | null>(null);
  const [applied, setApplied] = useState<{ before: PortfolioInput; label: string } | null>(null);

  // The latest text selection stays pinned while the user works in the panel;
  // a request from the canvas toolbar also switches the scope.
  const [seenText, setSeenText] = useState(textSelection);
  if (textSelection !== seenText) {
    setSeenText(textSelection);
    if (textSelection) setPinnedSelection(textSelection);
  }
  const [seenRequest, setSeenRequest] = useState(requested);
  if (requested !== seenRequest) {
    setSeenRequest(requested);
    if (requested) {
      setPinnedSelection(requested);
      setScope('selection');
    }
  }

  const selection = pinnedSelection;
  const sectionId =
    canvasSelection?.kind === 'section' ? canvasSelection.id : canvasSelection?.kind === 'entry' ? canvasSelection.sectionId : selection?.target.scope === 'section' || selection?.target.scope === 'entry' ? selection.target.sectionId : '';
  const section = draft.content.sections.find((s) => s.id === sectionId) ?? null;
  const effectiveScope: AiScope = isLetter && scope === 'section' ? 'document' : scope;

  const run = async () => {
    setBusy(true);
    setError(null);
    setPending(null);
    try {
      const proposal = await portfolioAiAssist({
        kind: draft.kind ?? 'cv',
        content: draft.content,
        letter: draft.letter ?? {},
        action,
        scope: effectiveScope,
        instruction,
        selection: effectiveScope === 'selection' ? (selection?.text ?? '') : '',
        sectionId: effectiveScope === 'section' ? sectionId : (selection?.target.scope === 'section' || selection?.target.scope === 'entry' ? selection.target.sectionId : ''),
        jobText: action === 'tailor' ? jobText : '',
        target,
        source: action === 'populate' ? source : null,
        sourceDocumentId: action === 'populate' && source === 'document' ? sourceDocumentId : null,
      });
      setPending({ proposal, request: { action, scope: effectiveScope, selection, sectionId } });
    } catch (err) {
      setError(toApiError(err).message);
    } finally {
      setBusy(false);
    }
  };

  const accept = () => {
    if (!pending) return;
    const { proposal, request } = pending;
    const before = draft;
    const label = ACTIONS.find((a) => a.action === request.action)?.label ?? 'AI change';
    edit((d) => {
      if (proposal.scope === 'selection' && proposal.text !== null && request.selection) {
        const current = getField(d, request.selection.target);
        return setField(d, request.selection.target, replaceSelection(current, request.selection.text, proposal.text));
      }
      if (proposal.scope === 'section' && proposal.section) {
        const next = proposal.section;
        return { ...d, content: { ...d.content, sections: d.content.sections.map((s) => (s.id === next.id ? { ...next, visible: s.visible } : s)) } };
      }
      if (proposal.scope === 'document' && proposal.content) {
        return { ...d, content: { ...proposal.content, header: { ...proposal.content.header, photo: d.content.header.photo ?? '' } } };
      }
      if (proposal.scope === 'document' && proposal.letter) {
        return { ...d, letter: proposal.letter };
      }
      return d;
    });
    setApplied({ before, label });
    setPending(null);
  };

  const undo = () => {
    if (!applied) return;
    const { before } = applied;
    edit(() => before);
    setApplied(null);
  };

  const selectionText = selection ? selection.text.trim() : '';
  const canRun =
    !busy &&
    (effectiveScope !== 'selection' || selectionText.length > 0) &&
    (effectiveScope !== 'section' || section !== null) &&
    (action !== 'custom' || instruction.trim().length > 0) &&
    (action !== 'tailor' || jobText.trim().length > 0) &&
    (action !== 'populate' || source === 'profile' || sourceDocumentId !== null);

  return (
    <div className="ai">
      <p className="ai__intro">
        Proposals use the model chosen in Settings and never add employers, dates, qualifications or figures that are not in your material. Figures that cannot be checked are flagged for you.
      </p>
      <div className="ai__scope">
        <span className="design__label">Apply to</span>
        <div className="segmented" role="radiogroup" aria-label="Scope">
          <button type="button" role="radio" aria-checked={effectiveScope === 'selection'} className={effectiveScope === 'selection' ? 'segmented__option segmented__option--active' : 'segmented__option'} onClick={() => setScope('selection')} disabled={!selectionText}>
            Selection
          </button>
          {!isLetter && (
            <button type="button" role="radio" aria-checked={effectiveScope === 'section'} className={effectiveScope === 'section' ? 'segmented__option segmented__option--active' : 'segmented__option'} onClick={() => setScope('section')} disabled={!section}>
              Section
            </button>
          )}
          <button type="button" role="radio" aria-checked={effectiveScope === 'document'} className={effectiveScope === 'document' ? 'segmented__option segmented__option--active' : 'segmented__option'} onClick={() => setScope('document')}>
            {isLetter ? 'Whole letter' : 'Whole CV'}
          </button>
        </div>
        <span className="form__hint">
          {effectiveScope === 'selection' && selectionText ? `“${selectionText.length > 90 ? `${selectionText.slice(0, 90)}…` : selectionText}”` : ''}
          {effectiveScope === 'section' && section ? `Section “${section.title}”` : ''}
          {effectiveScope === 'selection' && !selectionText ? 'Select text on the page to use this.' : ''}
          {effectiveScope === 'section' && !section ? 'Click a section on the page to use this.' : ''}
        </span>
      </div>

      <div className="ai__actions" role="radiogroup" aria-label="Action">
        {ACTIONS.filter((a) => !(isLetter && a.action === 'achievements')).map((a) => (
          <button key={a.action} type="button" role="radio" aria-checked={action === a.action} className={action === a.action ? 'ai__action ai__action--active' : 'ai__action'} onClick={() => setAction(a.action)}>
            <span className="ai__action-label">{a.label}</span>
            <span className="ai__action-hint">{a.hint}</span>
          </button>
        ))}
      </div>

      {action === 'tailor' && (
        <label className="field">
          <span className="field__label">Job posting</span>
          <textarea className="input input--textarea" rows={5} maxLength={20000} value={jobText} placeholder="Paste the job posting or its requirements." onChange={(e) => setJobText(e.target.value)} />
        </label>
      )}
      {(action === 'translate' || action === 'tone') && (
        <label className="field">
          <span className="field__label">{action === 'translate' ? 'Language' : 'Tone'}</span>
          <input className="input" maxLength={80} value={target} placeholder={action === 'translate' ? 'e.g. German' : 'e.g. confident and concise'} onChange={(e) => setTarget(e.target.value)} />
        </label>
      )}
      {action === 'populate' && (
        <fieldset className="field">
          <legend className="field__label">Fill in from</legend>
          <label className="radio">
            <input type="radio" name="ai-source" checked={source === 'profile'} onChange={() => setSource('profile')} />
            <span>My Custom Profile and credentials</span>
          </label>
          <label className="radio">
            <input type="radio" name="ai-source" checked={source === 'document'} onChange={() => setSource('document')} disabled={documents.length === 0} />
            <span>
              A document in my Profile (an older CV)
              {documents.length === 0 && <span className="form__hint"> None with readable text yet.</span>}
            </span>
          </label>
          {source === 'document' && documents.length > 0 && (
            <select className="input input--small" aria-label="Document" value={sourceDocumentId ?? ''} onChange={(e) => setSourceDocumentId(e.target.value ? Number(e.target.value) : null)}>
              <option value="">Choose a document…</option>
              {documents.map((d) => (
                <option key={d.id} value={d.id}>
                  {d.name}
                </option>
              ))}
            </select>
          )}
        </fieldset>
      )}
      <label className="field">
        <span className="field__label">{action === 'custom' ? 'Instruction' : 'Extra instruction (optional)'}</span>
        <textarea className="input input--textarea" rows={2} maxLength={2000} value={instruction} placeholder={action === 'custom' ? 'e.g. Make the summary sound less formal and mention remote work.' : 'e.g. Keep it under 60 words.'} onChange={(e) => setInstruction(e.target.value)} />
      </label>
      <button type="button" className="button button--primary" disabled={!canRun} onClick={() => void run()}>
        <SparkleIcon className="button__icon" />
        {busy ? 'Thinking…' : 'Propose'}
      </button>
      {error && (
        <p className="form-error" role="alert">
          {error}
        </p>
      )}

      {pending && (
        <div className="ai__proposal" role="region" aria-label="Proposal">
          <div className="ai__proposal-head">
            <strong>Proposal</strong>
            <span className="form__hint">by {pending.proposal.model}</span>
          </div>
          {pending.proposal.warnings.length > 0 && (
            <ul className="ai__warnings">
              {pending.proposal.warnings.map((w, i) => (
                <li key={i}>{w}</li>
              ))}
            </ul>
          )}
          {pending.proposal.scope === 'selection' && pending.proposal.text !== null && (
            <div className="ai__diff">
              <div className="ai__before">
                <span className="ai__diff-label">Before</span>
                {pending.request.selection?.text}
              </div>
              <div className="ai__after">
                <span className="ai__diff-label">After</span>
                {stripMarkup(pending.proposal.text)}
              </div>
            </div>
          )}
          {pending.proposal.scope === 'section' && pending.proposal.section && (
            <div className="ai__after">
              <span className="ai__diff-label">{pending.proposal.section.title}</span>
              {pending.proposal.section.text && <p>{stripMarkup(pending.proposal.section.text)}</p>}
              <ul className="ai__entries">
                {pending.proposal.section.entries.map((e) => (
                  <li key={e.id}>
                    <strong>{[e.title, e.subtitle].filter(Boolean).join(' · ')}</strong>
                    {e.description && <pre className="ai__pre">{stripMarkup(e.description)}</pre>}
                    {e.tags.length > 0 && <span className="form__hint">{e.tags.join(', ')}</span>}
                  </li>
                ))}
              </ul>
            </div>
          )}
          {pending.proposal.scope === 'document' && pending.proposal.content && (
            <div className="ai__after">
              <span className="ai__diff-label">Whole CV</span>
              <ul className="ai__entries">
                {pending.proposal.content.sections.map((s) => (
                  <li key={s.id}>
                    <strong>{s.title}</strong> <span className="form__hint">{s.entries.length ? `${s.entries.length} entries` : `${stripMarkup(s.text).length} characters`}</span>
                  </li>
                ))}
              </ul>
            </div>
          )}
          {pending.proposal.scope === 'document' && pending.proposal.letter && (
            <div className="ai__after">
              <span className="ai__diff-label">Letter</span>
              <pre className="ai__pre">{stripMarkup(pending.proposal.letter.body ?? '')}</pre>
            </div>
          )}
          {pending.proposal.notes.length > 0 && (
            <div className="ai__notes">
              <span className="ai__diff-label">Notes</span>
              <ul>
                {pending.proposal.notes.map((n, i) => (
                  <li key={i}>{n}</li>
                ))}
              </ul>
            </div>
          )}
          <div className="ai__proposal-actions">
            {(pending.proposal.text !== null || pending.proposal.section || pending.proposal.content || pending.proposal.letter) && pending.request.action !== 'suggest_missing' && (
              <button type="button" className="button button--primary button--small" onClick={accept}>
                Accept
              </button>
            )}
            <button type="button" className="button button--secondary button--small" onClick={() => setPending(null)}>
              {pending.request.action === 'suggest_missing' ? 'Done' : 'Reject'}
            </button>
          </div>
        </div>
      )}
      {applied && (
        <div className="ai__applied" role="status">
          <span>Applied: {applied.label}.</span>
          <button type="button" className="button button--ghost button--small" onClick={undo}>
            Undo
          </button>
        </div>
      )}
    </div>
  );
}
