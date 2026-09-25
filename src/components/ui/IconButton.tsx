import type { ButtonHTMLAttributes, ReactNode } from 'react';

interface IconButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  /** Accessible name and tooltip. */
  label: string;
  children: ReactNode;
}

/** A small square button containing only an icon. */
export function IconButton({ label, children, className, ...props }: IconButtonProps) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      className={className ? `icon-button ${className}` : 'icon-button'}
      {...props}
    >
      {children}
    </button>
  );
}
