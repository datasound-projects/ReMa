import { useMemo, useState } from 'react';

import { applyReview, reviewItems } from '../../lib/profileMerge';
import type { Profile, ProfileImport } from '../../services/profileService';
import { Dialog } from '../ui/Dialog';

interface ImportReviewDialogProps {
  current: Profile;
  result: ProfileImport;
  onClose: () => void;
  /** Receives the profile with the accepted changes. */
  onApply: (profile: Profile) => void;
}

/**
 * Shows what was read from a document. Gaps are pre-selected; values that
 * would replace something the user entered are not.
 */
export function ImportReviewDialog({ current, result, onClose, onApply }: ImportReviewDialogProps) {
  const items = useMemo(() => reviewItems(current, result.extracted), [current, result]);
  const [selected, setSelected] = useState(() => new Set(items.filter((i) => i.selected).map((i) => i.id)));
  const sections = [...new Set(items.map((i) => i.section))];

  const toggle = (id: string) =>
    setSelected((s) => {
      const next = new Set(s);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  return (
    <Dialog
      title={`Review “${result.document.name}”`}
      onClose={onClose}
      actions={
        <>
          <button type="button" className="button button--secondary" onClick={onClose}>
            Cancel
          </button>
          <button
            type="button"
            className="button button--primary"
            disabled={selected.size === 0}
            onClick={() => onApply(applyReview(current, result.extracted, selected))}
          >
            Add to Profile
          </button>
        </>
      }
    >
      <p className="dialog__text">
        {result.model
          ? `Read with ${result.model}. Check every value: nothing is saved until you add it.`
          : 'Check every value: nothing is saved until you add it.'}
      </p>
      {result.notes.map((note) => (
        <p key={note} className="form__hint review__note">
          {note}
        </p>
      ))}
      {items.length === 0 ? (
        <p className="dialog__text">Nothing new was found: your Profile already has these details.</p>
      ) : (
        <div className="review">
          {sections.map((section) => (
            <div key={section} className="review__section">
              <h3 className="review__heading">{section}</h3>
              {items
                .filter((i) => i.section === section)
                .map((item) => (
                  <label key={item.id} className="review__item">
                    <input type="checkbox" checked={selected.has(item.id)} onChange={() => toggle(item.id)} />
                    <span className="review__text">
                      <span className="review__label">{item.label}</span>
                      {item.value && <span className="review__value">{item.value}</span>}
                      {item.replaces && <span className="review__replaces">Replaces “{item.replaces}”</span>}
                    </span>
                  </label>
                ))}
            </div>
          ))}
        </div>
      )}
    </Dialog>
  );
}
