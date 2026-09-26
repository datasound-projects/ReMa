import { lazy, Suspense, type ComponentProps } from 'react';

import { LoadingState } from '../ui/EmptyState';

// PDF.js is large: it loads the first time a PDF is shown.
const PdfPages = lazy(() => import('./PdfPages').then((m) => ({ default: m.PdfPages })));

export function LazyPdfPages(props: ComponentProps<typeof PdfPages>) {
  return (
    <Suspense fallback={<LoadingState label="Loading the viewer…" />}>
      <PdfPages {...props} />
    </Suspense>
  );
}
