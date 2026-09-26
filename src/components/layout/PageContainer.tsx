import type { ReactNode } from 'react';

interface PageContainerProps {
  title: ReactNode;
  subtitle?: ReactNode;
  /** Buttons shown on the right of the title. */
  actions?: ReactNode;
  children?: ReactNode;
  /** `studio`: a wide work area (the Portfolio Studio editor). */
  width?: 'default' | 'wide' | 'studio';
}

/** Scrollable, width-constrained page body with a consistent header. */
export function PageContainer({ title, subtitle, actions, children, width = 'default' }: PageContainerProps) {
  return (
    <div className="page">
      <div className={width === 'default' ? 'page__inner' : `page__inner page__inner--${width}`}>
        <header className="page__header">
          <div className="page__heading">
            <h1 className="page__title">{title}</h1>
            {subtitle && <p className="page__subtitle">{subtitle}</p>}
          </div>
          {actions && <div className="page__actions">{actions}</div>}
        </header>
        {children}
      </div>
    </div>
  );
}
