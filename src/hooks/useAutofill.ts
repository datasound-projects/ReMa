import { useState } from 'react';

import { runAutofill, type AutofillResult } from '../services/browserService';
import { toApiError } from '../services/ipc';

interface AutofillState {
  /** The page the result belongs to. */
  url: string | null;
  running: boolean;
  result: AutofillResult | null;
  error: string | null;
}

const idle = (url: string | null): AutofillState => ({ url, running: false, result: null, error: null });

/** ReMa Auto Fill for the page currently in the browser. */
export function useAutofill(url: string | null) {
  const [state, setState] = useState<AutofillState>(() => idle(url));
  // A new page: the previous result no longer applies.
  if (state.url !== url) setState(idle(url));

  const run = async () => {
    setState((s) => ({ ...s, running: true, error: null }));
    try {
      const result = await runAutofill();
      setState((s) => (s.url === url ? { ...s, running: false, result } : s));
    } catch (err) {
      setState((s) => (s.url === url ? { ...s, running: false, error: toApiError(err).message } : s));
    }
  };

  return {
    ...state,
    run,
    dismiss: () => setState(idle(url)),
  };
}

export type Autofill = ReturnType<typeof useAutofill>;
