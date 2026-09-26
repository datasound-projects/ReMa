import { useEffect, useMemo, useState } from 'react';

import type { LayoutInput } from '../lib/portfolio/layout';

interface PdfState {
  data: Uint8Array | null;
  rendering: boolean;
  error: string | null;
}

interface Rendered {
  /** The input the last result belongs to. */
  key: string | null;
  data: Uint8Array | null;
  error: string | null;
}

/**
 * Renders the document to PDF shortly after it stops changing. The last
 * good PDF stays available while a new one renders.
 */
export function usePortfolioPdf(input: LayoutInput | null, delay = 350): PdfState {
  const key = useMemo(() => (input ? JSON.stringify(input) : null), [input]);
  const [result, setResult] = useState<Rendered>({ key: null, data: null, error: null });

  useEffect(() => {
    if (key === null) return;
    let cancelled = false;
    const timer = window.setTimeout(() => {
      void (async () => {
        try {
          const { renderPdf } = await import('../lib/portfolio/pdf');
          const data = await renderPdf(JSON.parse(key) as LayoutInput);
          if (!cancelled) setResult({ key, data, error: null });
        } catch (error) {
          if (!cancelled) {
            setResult((r) => ({
              key,
              data: r.data,
              error: error instanceof Error ? error.message : 'The preview could not be rendered.',
            }));
          }
        }
      })();
    }, delay);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [key, delay]);

  return { data: result.data, error: result.error, rendering: key !== null && result.key !== key };
}
