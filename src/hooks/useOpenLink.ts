import type { MouseEvent } from 'react';

import { useOptionalBrowser } from '../app/browser';
import { openExternalUrl } from '../services/systemService';

/** Opens web links like the rest of ReMa: in the ReMa browser, or externally with Ctrl/⌘. */
export function useOpenLink() {
  const browser = useOptionalBrowser();
  return (url: string, event?: MouseEvent) => {
    if (event && (event.metaKey || event.ctrlKey || event.shiftKey)) {
      void openExternalUrl(url).catch(() => {});
    } else if (browser) {
      browser.openUrl(url);
    } else {
      void openExternalUrl(url).catch(() => {});
    }
  };
}

