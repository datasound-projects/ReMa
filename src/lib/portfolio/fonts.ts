/**
 * Fonts embedded in Portfolio Studio PDFs (all SIL Open Font License).
 * Keys are the names inside pdfmake's virtual file system; the browser
 * loads them from ReMa's own assets (`fontUrls.ts`), tests from disk.
 */
export const FONT_FILES = {
  'Inter-Regular.ttf': '@expo-google-fonts/inter/400Regular/Inter_400Regular.ttf',
  'Inter-Italic.ttf': '@expo-google-fonts/inter/400Regular_Italic/Inter_400Regular_Italic.ttf',
  'Inter-SemiBold.ttf': '@expo-google-fonts/inter/600SemiBold/Inter_600SemiBold.ttf',
  'Inter-Bold.ttf': '@expo-google-fonts/inter/700Bold/Inter_700Bold.ttf',
  'Inter-BoldItalic.ttf': '@expo-google-fonts/inter/700Bold_Italic/Inter_700Bold_Italic.ttf',
  'SourceSerif-Regular.ttf': '@expo-google-fonts/source-serif-4/400Regular/SourceSerif4_400Regular.ttf',
  'SourceSerif-Italic.ttf': '@expo-google-fonts/source-serif-4/400Regular_Italic/SourceSerif4_400Regular_Italic.ttf',
  'SourceSerif-Bold.ttf': '@expo-google-fonts/source-serif-4/700Bold/SourceSerif4_700Bold.ttf',
  'SourceSerif-BoldItalic.ttf': '@expo-google-fonts/source-serif-4/700Bold_Italic/SourceSerif4_700Bold_Italic.ttf',
  'SourceSans-Regular.ttf': '@expo-google-fonts/source-sans-3/400Regular/SourceSans3_400Regular.ttf',
  'SourceSans-Italic.ttf': '@expo-google-fonts/source-sans-3/400Regular_Italic/SourceSans3_400Regular_Italic.ttf',
  'SourceSans-SemiBold.ttf': '@expo-google-fonts/source-sans-3/600SemiBold/SourceSans3_600SemiBold.ttf',
  'SourceSans-Bold.ttf': '@expo-google-fonts/source-sans-3/700Bold/SourceSans3_700Bold.ttf',
  'SourceSans-BoldItalic.ttf': '@expo-google-fonts/source-sans-3/700Bold_Italic/SourceSans3_700Bold_Italic.ttf',
  'CodePro-Regular.ttf': '@expo-google-fonts/source-code-pro/400Regular/SourceCodePro_400Regular.ttf',
  'CodePro-Bold.ttf': '@expo-google-fonts/source-code-pro/700Bold/SourceCodePro_700Bold.ttf',
  'Playfair-Regular.ttf': '@expo-google-fonts/playfair-display/400Regular/PlayfairDisplay_400Regular.ttf',
  'Playfair-Italic.ttf': '@expo-google-fonts/playfair-display/400Regular_Italic/PlayfairDisplay_400Regular_Italic.ttf',
  'Playfair-Bold.ttf': '@expo-google-fonts/playfair-display/700Bold/PlayfairDisplay_700Bold.ttf',
} as const;

export type FontFile = keyof typeof FONT_FILES;

interface FontFamily {
  normal: FontFile;
  bold: FontFile;
  italics: FontFile;
  bolditalics: FontFile;
}

/** Families the templates use. "…Semi" families set semibold as normal. */
export const FONT_FAMILIES = {
  Inter: {
    normal: 'Inter-Regular.ttf',
    bold: 'Inter-Bold.ttf',
    italics: 'Inter-Italic.ttf',
    bolditalics: 'Inter-BoldItalic.ttf',
  },
  InterSemi: {
    normal: 'Inter-SemiBold.ttf',
    bold: 'Inter-Bold.ttf',
    italics: 'Inter-Italic.ttf',
    bolditalics: 'Inter-BoldItalic.ttf',
  },
  SourceSerif: {
    normal: 'SourceSerif-Regular.ttf',
    bold: 'SourceSerif-Bold.ttf',
    italics: 'SourceSerif-Italic.ttf',
    bolditalics: 'SourceSerif-BoldItalic.ttf',
  },
  SourceSans: {
    normal: 'SourceSans-Regular.ttf',
    bold: 'SourceSans-Bold.ttf',
    italics: 'SourceSans-Italic.ttf',
    bolditalics: 'SourceSans-BoldItalic.ttf',
  },
  SourceSansSemi: {
    normal: 'SourceSans-SemiBold.ttf',
    bold: 'SourceSans-Bold.ttf',
    italics: 'SourceSans-Italic.ttf',
    bolditalics: 'SourceSans-BoldItalic.ttf',
  },
  CodeMono: {
    normal: 'CodePro-Regular.ttf',
    bold: 'CodePro-Bold.ttf',
    italics: 'CodePro-Regular.ttf',
    bolditalics: 'CodePro-Bold.ttf',
  },
  Playfair: {
    normal: 'Playfair-Regular.ttf',
    bold: 'Playfair-Bold.ttf',
    italics: 'Playfair-Italic.ttf',
    bolditalics: 'Playfair-Bold.ttf',
  },
} satisfies Record<string, FontFamily>;

export type FontName = keyof typeof FONT_FAMILIES;
