import type { ReactNode } from 'react';

interface PageContainerProps {
  title: ReactNode;
  subtitle?: ReactNode;
  /** Buttons shown on the right of the title. */
  actions?: ReactNode;
  children?: ReactNode;
}

/** Scrollable, width-constrained page body with a consistent header. */
export function PageContainer({ title, subtitle, actions, children }: PageContainerProps) {
  return (
    <div className="page">
      <div className="page__inner">
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
