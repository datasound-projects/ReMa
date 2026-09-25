import type { ReactNode } from 'react';

interface PageContainerProps {
  title: string;
  subtitle?: string;
  children?: ReactNode;
}

/** Scrollable, width-constrained page body with a consistent header. */
export function PageContainer({ title, subtitle, children }: PageContainerProps) {
  return (
    <div className="page">
      <div className="page__inner">
        <header className="page__header">
          <h1 className="page__title">{title}</h1>
          {subtitle && <p className="page__subtitle">{subtitle}</p>}
        </header>
        {children}
      </div>
    </div>
  );
}
