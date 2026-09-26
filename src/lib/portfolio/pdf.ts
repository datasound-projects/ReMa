/**
 * Renders Portfolio Studio documents to PDF in the interface with pdfmake
 * (real text, embedded fonts). Loaded on demand: only Portfolio Studio
 * needs it.
 */
import * as pdfMakeModule from 'pdfmake/build/pdfmake';

import { FONT_FAMILIES, type FontFile } from './fonts';
import { FONT_URLS } from './fontUrls';
import { buildDocument, type LayoutInput } from './layout';

type PdfMake = typeof pdfMakeModule;
// The browser build is a CommonJS bundle: its API is the default export.
const pdfMake: PdfMake = (pdfMakeModule as unknown as { default?: PdfMake }).default ?? pdfMakeModule;

function toBase64(bytes: Uint8Array): string {
  let binary = '';
  const chunk = 0x8000;
  for (let i = 0; i < bytes.length; i += chunk) {
    binary += String.fromCharCode(...bytes.subarray(i, i + chunk));
  }
  return btoa(binary);
}

let fontsReady: Promise<void> | null = null;

/** Loads the bundled fonts into pdfmake once. */
function loadFonts(): Promise<void> {
  fontsReady ??= (async () => {
    const entries = Object.entries(FONT_URLS) as [FontFile, string][];
    const files = await Promise.all(
      entries.map(async ([name, url]) => {
        const response = await fetch(url);
        if (!response.ok) throw new Error(`Font ${name} is missing (${response.status}).`);
        return [name, toBase64(new Uint8Array(await response.arrayBuffer()))] as const;
      }),
    );
    pdfMake.addVirtualFileSystem(Object.fromEntries(files));
    pdfMake.setFonts(FONT_FAMILIES);
  })().catch((error: unknown) => {
    fontsReady = null;
    throw error;
  });
  return fontsReady;
}

/** The document as PDF bytes (the same file for preview and export). */
export async function renderPdf(input: LayoutInput): Promise<Uint8Array> {
  await loadFonts();
  const buffer = await pdfMake.createPdf(buildDocument(input)).getBuffer();
  return new Uint8Array(buffer);
}

export { toBase64 };
