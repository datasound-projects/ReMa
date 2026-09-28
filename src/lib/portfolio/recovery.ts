/**
 * Autosave recovery: the latest unsaved draft of a document is kept in
 * the browser's storage until it is saved, so a crash or a closed window
 * loses nothing. Drafts are small JSON documents; nothing else is kept.
 */
import type { PortfolioInput } from '../../services/portfolioService';

const PREFIX = 'rema.portfolio.draft.';

export interface RecoveredDraft {
  input: PortfolioInput;
  savedAt: number;
}

export function keepDraft(id: number, input: PortfolioInput): void {
  try {
    localStorage.setItem(`${PREFIX}${id}`, JSON.stringify({ input, savedAt: Date.now() } satisfies RecoveredDraft));
  } catch {
    // Storage may be full or unavailable; autosave to the backend still runs.
  }
}

export function clearDraft(id: number): void {
  try {
    localStorage.removeItem(`${PREFIX}${id}`);
  } catch {
    // Nothing to do.
  }
}

/** A draft newer than the stored document, if any. */
export function recoverDraft(id: number, updatedAt: number): RecoveredDraft | null {
  try {
    const raw = localStorage.getItem(`${PREFIX}${id}`);
    if (!raw) return null;
    const draft = JSON.parse(raw) as RecoveredDraft;
    if (!draft || typeof draft.savedAt !== 'number' || !draft.input || draft.savedAt <= updatedAt) {
      localStorage.removeItem(`${PREFIX}${id}`);
      return null;
    }
    return draft;
  } catch {
    return null;
  }
}
