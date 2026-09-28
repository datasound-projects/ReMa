/**
 * The document canvas uses the same font files as the PDF, loaded once
 * with the FontFace API, so what is edited looks like what is exported.
 */
import { FONT_FAMILIES, type FontFile, type FontName } from './fonts';
import { FONT_URLS } from './fontUrls';

let loading: Promise<void> | null = null;

/** CSS font-family names used on the canvas, per template font. */
export const CANVAS_FAMILY: Record<FontName, string> = {
  Inter: 'ReMa Inter',
  InterSemi: 'ReMa Inter',
  SourceSerif: 'ReMa Source Serif',
  SourceSans: 'ReMa Source Sans',
  SourceSansSemi: 'ReMa Source Sans',
  CodeMono: 'ReMa Source Code Pro',
  Playfair: 'ReMa Playfair Display',
};

/** Font weight the "normal" face of a template font stands for. */
export const CANVAS_WEIGHT: Record<FontName, number> = {
  Inter: 400,
  InterSemi: 600,
  SourceSerif: 400,
  SourceSans: 400,
  SourceSansSemi: 600,
  CodeMono: 400,
  Playfair: 400,
};

const FACES: { family: string; file: FontFile; weight: number; style: 'normal' | 'italic' }[] = [
  { family: 'ReMa Inter', file: 'Inter-Regular.ttf', weight: 400, style: 'normal' },
  { family: 'ReMa Inter', file: 'Inter-Italic.ttf', weight: 400, style: 'italic' },
  { family: 'ReMa Inter', file: 'Inter-SemiBold.ttf', weight: 600, style: 'normal' },
  { family: 'ReMa Inter', file: 'Inter-Bold.ttf', weight: 700, style: 'normal' },
  { family: 'ReMa Inter', file: 'Inter-BoldItalic.ttf', weight: 700, style: 'italic' },
  { family: 'ReMa Source Serif', file: 'SourceSerif-Regular.ttf', weight: 400, style: 'normal' },
  { family: 'ReMa Source Serif', file: 'SourceSerif-Italic.ttf', weight: 400, style: 'italic' },
  { family: 'ReMa Source Serif', file: 'SourceSerif-Bold.ttf', weight: 700, style: 'normal' },
  { family: 'ReMa Source Serif', file: 'SourceSerif-BoldItalic.ttf', weight: 700, style: 'italic' },
  { family: 'ReMa Source Sans', file: 'SourceSans-Regular.ttf', weight: 400, style: 'normal' },
  { family: 'ReMa Source Sans', file: 'SourceSans-Italic.ttf', weight: 400, style: 'italic' },
  { family: 'ReMa Source Sans', file: 'SourceSans-SemiBold.ttf', weight: 600, style: 'normal' },
  { family: 'ReMa Source Sans', file: 'SourceSans-Bold.ttf', weight: 700, style: 'normal' },
  { family: 'ReMa Source Sans', file: 'SourceSans-BoldItalic.ttf', weight: 700, style: 'italic' },
  { family: 'ReMa Source Code Pro', file: 'CodePro-Regular.ttf', weight: 400, style: 'normal' },
  { family: 'ReMa Source Code Pro', file: 'CodePro-Bold.ttf', weight: 700, style: 'normal' },
  { family: 'ReMa Playfair Display', file: 'Playfair-Regular.ttf', weight: 400, style: 'normal' },
  { family: 'ReMa Playfair Display', file: 'Playfair-Italic.ttf', weight: 400, style: 'italic' },
  { family: 'ReMa Playfair Display', file: 'Playfair-Bold.ttf', weight: 700, style: 'normal' },
];

/** Registers the document fonts with the page once. */
export function loadCanvasFonts(): Promise<void> {
  if (typeof FontFace === 'undefined' || typeof document === 'undefined') return Promise.resolve();
  loading ??= (async () => {
    await Promise.all(
      FACES.map(async (face) => {
        const font = new FontFace(face.family, `url(${FONT_URLS[face.file]})`, { weight: String(face.weight), style: face.style });
        try {
          document.fonts.add(await font.load());
        } catch {
          // The canvas falls back to system fonts for that face.
        }
      }),
    );
  })();
  return loading;
}

// Keeps the font dictionary referenced so the two lists cannot drift apart.
export const FONT_FILE_COUNT = Object.keys(FONT_FAMILIES).length;
