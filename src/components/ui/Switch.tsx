import type { ReactNode } from 'react';

interface SwitchProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  /** Visible label; pass `aria-label` instead for a switch without one. */
  children?: ReactNode;
  'aria-label'?: string;
  disabled?: boolean;
}

/** An on/off setting that takes effect immediately (a checkbox underneath). */
export function Switch({ checked, onChange, children, disabled, 'aria-label': ariaLabel }: SwitchProps) {
  return (
    <label className="switch">
      <input
        type="checkbox"
        role="switch"
        className="switch__input"
        checked={checked}
        disabled={disabled}
        aria-label={ariaLabel}
        onChange={(event) => onChange(event.target.checked)}
      />
      <span className="switch__track" aria-hidden="true" />
      {children}
    </label>
  );
}
