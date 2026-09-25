import { useState } from 'react';

import { useOptionalAnalytics } from '../../app/analytics';
import { toApiError } from '../../services/ipc';
import type { LinkedRun } from '../../services/analyticsService';
import { ChartIcon } from '../icons';

/**
 * "Analyze" on a chat answer or task result: opens Analytics with that
 * search selected. Answers ReMa already read open directly; others are read
 * first (a job list written as text needs the model).
 */
export function AnalyzeButton({ run, analyze }: { run: LinkedRun | undefined; analyze: () => Promise<number> }) {
  const analytics = useOptionalAnalytics();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  if (!analytics) return null;
  const onClick = () => {
    setError(null);
    if (run) {
      analytics.openAnalytics({ runIds: [run.runId], tab: 'jobs' });
      return;
    }
    setBusy(true);
    analyze()
      .then((runId) => analytics.openAnalytics({ runIds: [runId], tab: 'jobs' }))
      .catch((err: unknown) => setError(toApiError(err).message))
      .finally(() => setBusy(false));
  };
  return (
    <>
      <button
        type="button"
        className="analyze-button"
        disabled={busy}
        title={run ? 'Open these jobs in Analytics' : 'Read the jobs in this answer into Analytics'}
        onClick={onClick}
      >
        <ChartIcon className="analyze-button__icon" />
        {busy ? 'Reading jobs…' : run ? `Analyze ${run.jobs} job${run.jobs === 1 ? '' : 's'}` : 'Analyze jobs'}
      </button>
      {error && <span className="analyze-button__error">{error}</span>}
    </>
  );
}
