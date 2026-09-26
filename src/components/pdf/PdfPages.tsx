import { useEffect, useRef, useState } from 'react';

import { openPdf, type PDFDocumentProxy } from '../../lib/pdf/pdfjs';

interface PdfPagesProps {
  /** The PDF bytes. A new array renders the new document in place. */
  data: Uint8Array | null;
  /** 1 = pages fill the container's width. */
  zoom?: number;
  /** Accessible name of the pages, e.g. "Preview of My CV". */
  label: string;
  /** Called once the document is shown (page count), or when it fails. */
  onRendered?: (pages: number) => void;
  onError?: (message: string) => void;
}

/** Documents longer than this appear page by page instead of all at once. */
const SWAP_AFTER_PAGES = 3;
const MAX_CANVAS_PIXELS = 16_000_000;

async function renderPage(pdf: PDFDocumentProxy, number: number, cssWidth: number): Promise<HTMLCanvasElement> {
  const page = await pdf.getPage(number);
  const base = page.getViewport({ scale: 1 });
  const ratio = window.devicePixelRatio || 1;
  let scale = (cssWidth / base.width) * ratio;
  // Very large pages at high zoom: keep the canvas within memory limits.
  const pixels = base.width * base.height * scale * scale;
  if (pixels > MAX_CANVAS_PIXELS) scale *= Math.sqrt(MAX_CANVAS_PIXELS / pixels);
  const viewport = page.getViewport({ scale });
  const canvas = document.createElement('canvas');
  canvas.width = Math.floor(viewport.width);
  canvas.height = Math.floor(viewport.height);
  canvas.className = 'pdf-pages__page';
  canvas.style.width = `${cssWidth}px`;
  canvas.style.aspectRatio = `${base.width} / ${base.height}`;
  canvas.setAttribute('role', 'img');
  canvas.setAttribute('aria-label', `Page ${number} of ${pdf.numPages}`);
  await page.render({ canvas, viewport }).promise;
  page.cleanup();
  return canvas;
}

/**
 * Renders every page of a PDF to canvases that fit the container. When the
 * data changes, the old pages stay until the new ones are ready (no flicker
 * while editing).
 */
export function PdfPages({ data, zoom = 1, label, onRendered, onError }: PdfPagesProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(0);
  const callbacks = useRef({ onRendered, onError });
  useEffect(() => {
    callbacks.current = { onRendered, onError };
  });

  // Follow the container's width (debounced while resizing).
  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    let timer = 0;
    const observer = new ResizeObserver((entries) => {
      const next = Math.floor(entries[0]?.contentRect.width ?? 0);
      window.clearTimeout(timer);
      timer = window.setTimeout(() => setWidth((w) => (Math.abs(w - next) > 1 ? next : w)), 120);
    });
    observer.observe(el);
    setWidth(Math.floor(el.clientWidth));
    return () => {
      observer.disconnect();
      window.clearTimeout(timer);
    };
  }, []);

  useEffect(() => {
    const container = containerRef.current;
    if (!container || !data || width <= 0) return;
    let cancelled = false;
    // PDF.js takes ownership of the array; keep the caller's copy intact.
    const task = openPdf(data.slice());
    const cssWidth = Math.max(120, Math.floor(width * zoom));

    void (async () => {
      let pdf: PDFDocumentProxy | null = null;
      try {
        pdf = await task.promise;
        const canvases: HTMLCanvasElement[] = [];
        for (let n = 1; n <= pdf.numPages; n++) {
          const canvas = await renderPage(pdf, n, cssWidth);
          if (cancelled) return;
          canvases.push(canvas);
          if (n === Math.min(SWAP_AFTER_PAGES, pdf.numPages)) container.replaceChildren(...canvases);
          else if (n > SWAP_AFTER_PAGES) container.append(canvas);
        }
        if (!cancelled) callbacks.current.onRendered?.(pdf.numPages);
      } catch (error) {
        if (cancelled) return;
        const message = error instanceof Error ? error.message : String(error);
        callbacks.current.onError?.(
          /password/i.test(message)
            ? 'This PDF is password-protected, so ReMa cannot show it.'
            : 'This PDF could not be shown.',
        );
      } finally {
        // Frees the worker's copy of the document.
        if (pdf) void task.destroy();
      }
    })();

    return () => {
      cancelled = true;
      void task.destroy();
    };
  }, [data, width, zoom]);

  return <div ref={containerRef} className="pdf-pages" role="group" aria-label={label} />;
}
