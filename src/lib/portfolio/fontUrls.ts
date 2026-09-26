// Font files bundled with ReMa (see fonts.ts). Vite copies each into the
// build and gives its URL; nothing is loaded from the web.
import interBold from '@expo-google-fonts/inter/700Bold/Inter_700Bold.ttf?url';
import interBoldItalic from '@expo-google-fonts/inter/700Bold_Italic/Inter_700Bold_Italic.ttf?url';
import interRegular from '@expo-google-fonts/inter/400Regular/Inter_400Regular.ttf?url';
import interItalic from '@expo-google-fonts/inter/400Regular_Italic/Inter_400Regular_Italic.ttf?url';
import interSemiBold from '@expo-google-fonts/inter/600SemiBold/Inter_600SemiBold.ttf?url';
import codeProBold from '@expo-google-fonts/source-code-pro/700Bold/SourceCodePro_700Bold.ttf?url';
import codeProRegular from '@expo-google-fonts/source-code-pro/400Regular/SourceCodePro_400Regular.ttf?url';
import playfairBold from '@expo-google-fonts/playfair-display/700Bold/PlayfairDisplay_700Bold.ttf?url';
import playfairRegular from '@expo-google-fonts/playfair-display/400Regular/PlayfairDisplay_400Regular.ttf?url';
import playfairItalic from '@expo-google-fonts/playfair-display/400Regular_Italic/PlayfairDisplay_400Regular_Italic.ttf?url';
import sourceSansBold from '@expo-google-fonts/source-sans-3/700Bold/SourceSans3_700Bold.ttf?url';
import sourceSansBoldItalic from '@expo-google-fonts/source-sans-3/700Bold_Italic/SourceSans3_700Bold_Italic.ttf?url';
import sourceSansRegular from '@expo-google-fonts/source-sans-3/400Regular/SourceSans3_400Regular.ttf?url';
import sourceSansItalic from '@expo-google-fonts/source-sans-3/400Regular_Italic/SourceSans3_400Regular_Italic.ttf?url';
import sourceSansSemiBold from '@expo-google-fonts/source-sans-3/600SemiBold/SourceSans3_600SemiBold.ttf?url';
import sourceSerifBold from '@expo-google-fonts/source-serif-4/700Bold/SourceSerif4_700Bold.ttf?url';
import sourceSerifBoldItalic from '@expo-google-fonts/source-serif-4/700Bold_Italic/SourceSerif4_700Bold_Italic.ttf?url';
import sourceSerifRegular from '@expo-google-fonts/source-serif-4/400Regular/SourceSerif4_400Regular.ttf?url';
import sourceSerifItalic from '@expo-google-fonts/source-serif-4/400Regular_Italic/SourceSerif4_400Regular_Italic.ttf?url';

import type { FontFile } from './fonts';

export const FONT_URLS: Record<FontFile, string> = {
  'Inter-Regular.ttf': interRegular,
  'Inter-Italic.ttf': interItalic,
  'Inter-SemiBold.ttf': interSemiBold,
  'Inter-Bold.ttf': interBold,
  'Inter-BoldItalic.ttf': interBoldItalic,
  'SourceSerif-Regular.ttf': sourceSerifRegular,
  'SourceSerif-Italic.ttf': sourceSerifItalic,
  'SourceSerif-Bold.ttf': sourceSerifBold,
  'SourceSerif-BoldItalic.ttf': sourceSerifBoldItalic,
  'SourceSans-Regular.ttf': sourceSansRegular,
  'SourceSans-Italic.ttf': sourceSansItalic,
  'SourceSans-SemiBold.ttf': sourceSansSemiBold,
  'SourceSans-Bold.ttf': sourceSansBold,
  'SourceSans-BoldItalic.ttf': sourceSansBoldItalic,
  'CodePro-Regular.ttf': codeProRegular,
  'CodePro-Bold.ttf': codeProBold,
  'Playfair-Regular.ttf': playfairRegular,
  'Playfair-Italic.ttf': playfairItalic,
  'Playfair-Bold.ttf': playfairBold,
};
