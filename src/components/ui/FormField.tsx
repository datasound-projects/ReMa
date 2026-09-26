import { useId, type ReactNode } from 'react';

interface FormFieldProps {
  label: string;
  /** Shown below the control and read as its description. */
  hint?: ReactNode;
  grow?: boolean;
  /** Renders the control with the ids that connect it to its label and hint. */
  children: (control: { id: string; 'aria-describedby'?: string }) => ReactNode;
}

/** A labelled form control with an optional hint. */
export function FormField({ label, hint, grow, children }: FormFieldProps) {
  const id = useId();
  const hintId = `${id}-hint`;
  return (
    <div className={grow ? 'field field--grow' : 'field'}>
      <label className="field__label" htmlFor={id}>
        {label}
      </label>
      {children({ id, ...(hint ? { 'aria-describedby': hintId } : {}) })}
      {hint && (
        <span className="form__hint" id={hintId}>
          {hint}
        </span>
      )}
    </div>
  );
}
