import { useId, useState } from 'react';

import { QuestionIcon } from '../icons';

/**
 * A small "?" that shows one short sentence on hover or keyboard focus.
 * It never opens anything else.
 */
export function HelpTip({ text, label = 'What is this?' }: { text: string; label?: string }) {
  const id = useId();
  const [open, setOpen] = useState(false);
  return (
    <span
      className="help-tip"
      onMouseEnter={() => setOpen(true)}
      onMouseLeave={() => setOpen(false)}
    >
      <button
        type="button"
        className="help-tip__button"
        aria-label={label}
        aria-describedby={id}
        onFocus={() => setOpen(true)}
        onBlur={() => setOpen(false)}
        onKeyDown={(e) => {
          if (e.key === 'Escape') setOpen(false);
        }}
        onClick={() => setOpen((o) => !o)}
      >
        <QuestionIcon aria-hidden="true" />
      </button>
      <span id={id} role="tooltip" className={open ? 'help-tip__bubble help-tip__bubble--open' : 'help-tip__bubble'}>
        {text}
      </span>
    </span>
  );
}
