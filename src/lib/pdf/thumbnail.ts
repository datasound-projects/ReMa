import { openPdf } from './pdfjs';

/** The first page of a PDF as a PNG data URL, `width` CSS pixels wide. */
export async function firstPageImage(data: Uint8Array, width: number): Promise<string> {
  const task = openPdf(data.slice());
  try {
    const pdf = await task.promise;
    const page = await pdf.getPage(1);
    const base = page.getViewport({ scale: 1 });
    const scale = (width * Math.min(window.devicePixelRatio || 1, 2)) / base.width;
    const viewport = page.getViewport({ scale });
    const canvas = document.createElement('canvas');
    canvas.width = Math.floor(viewport.width);
    canvas.height = Math.floor(viewport.height);
    await page.render({ canvas, viewport }).promise;
    return canvas.toDataURL('image/png');
  } finally {
    void task.destroy();
  }
}
