import { useEffect, useRef, type ReactNode } from 'react';

import { useCoverBrowser } from '../../app/browser';

interface DialogProps {
  title: string;
  onClose: () => void;
  children: ReactNode;
  /** Footer buttons. */
  actions: ReactNode;
  /** `wide` for forms with two columns or long content. */
  size?: 'default' | 'wide';
}

/** A small modal dialog built on the native `<dialog>` element. */
export function Dialog({ title, onClose, children, actions, size = 'default' }: DialogProps) {
  const ref = useRef<HTMLDialogElement>(null);
  // Web pages are drawn natively above ReMa; hide the page meanwhile.
  useCoverBrowser();

  useEffect(() => {
    const dialog = ref.current;
    if (dialog && !dialog.open) dialog.showModal();
  }, []);

  return (
    <dialog
      ref={ref}
      className={size === 'wide' ? 'dialog dialog--wide' : 'dialog'}
      aria-label={title}
      onCancel={(event) => {
        event.preventDefault();
        onClose();
      }}
      onMouseDown={(event) => {
        // Click on the backdrop (the dialog element itself) closes it.
        if (event.target === ref.current) onClose();
      }}
    >
      <div className="dialog__body">
        <h2 className="dialog__title">{title}</h2>
        {children}
      </div>
      <div className="dialog__actions">{actions}</div>
    </dialog>
  );
}
