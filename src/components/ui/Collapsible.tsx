import { useId, useState, type ReactNode } from 'react';

import { ChevronRightIcon } from '../icons';

interface CollapsibleProps {
  title: string;
  /** Shown after the title ("Outputs 2"). */
  count?: number;
  defaultOpen?: boolean;
  children: ReactNode;
}

/** A titled section that opens and closes (compact inspector sections). */
export function Collapsible({ title, count, defaultOpen = false, children }: CollapsibleProps) {
  const [open, setOpen] = useState(defaultOpen);
  const id = useId();
  return (
    <section className={open ? 'collapsible collapsible--open' : 'collapsible'}>
      <h3 className="collapsible__heading">
        <button
          type="button"
          className="collapsible__toggle"
          aria-expanded={open}
          aria-controls={id}
          onClick={() => setOpen(!open)}
        >
          <ChevronRightIcon className="collapsible__chevron" />
          <span className="collapsible__title">{title}</span>
          {count !== undefined && <span className="collapsible__count">{count}</span>}
        </button>
      </h3>
      <div id={id} className="collapsible__body" hidden={!open}>
        {children}
      </div>
    </section>
  );
}
