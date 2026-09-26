/**
 * PDF.js, set up for ReMa: the legacy build (older macOS WebKit), a local
 * worker, and fonts, character maps and decoders served by ReMa itself
 * (see `pdfjsAssets` in vite.config.ts). Nothing is fetched from the web.
 */
import {
  AnnotationMode,
  GlobalWorkerOptions,
  getDocument,
  type PDFDocumentLoadingTask,
} from 'pdfjs-dist/legacy/build/pdf.mjs';
import workerUrl from 'pdfjs-dist/legacy/build/pdf.worker.min.mjs?url';

export type { PDFDocumentProxy, PDFPageProxy } from 'pdfjs-dist/legacy/build/pdf.mjs';
export { AnnotationMode };

GlobalWorkerOptions.workerSrc = workerUrl;

const assets = (folder: string) => new URL(`pdfjs/${folder}/`, document.baseURI).href;

/** Opens PDF bytes (the array is transferred to the worker). */
export function openPdf(data: Uint8Array): PDFDocumentLoadingTask {
  return getDocument({
    data,
    cMapUrl: assets('cmaps'),
    standardFontDataUrl: assets('standard_fonts'),
    iccUrl: assets('iccs'),
    wasmUrl: assets('wasm'),
    // The main thread reads those files: custom schemes are not
    // reliably reachable from workers in every webview.
    useWorkerFetch: false,
    // Documents are shown, never run: no forms, no scripts.
    enableXfa: false,
    stopAtErrors: false,
  });
}
