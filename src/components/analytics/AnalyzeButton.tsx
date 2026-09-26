import { useState } from 'react';

import { useOptionalAnalytics } from '../../app/analytics';
import { toApiError } from '../../services/ipc';
import type { LinkedRun } from '../../services/analyticsService';
import { ChartIcon } from '../icons';
import { LinkedText } from '../ui/LinkedText';

interface Note {
  text: string;
  /** A real failure (red) rather than an explanation. */
  error: boolean;
}

/**
 * "Analyze" on a chat answer or task result: adds the job postings it lists
 * to Analytics (ranking, skill gaps, requirements) and opens it with that
 * search selected. Answers with a job table were already added when they
 * finished; others are read first, with the model that wrote them.
 */
export function AnalyzeButton({ run, analyze }: { run: LinkedRun | undefined; analyze: () => Promise<number> }) {
  const analytics = useOptionalAnalytics();
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<Note | null>(null);
  if (!analytics) return null;
  const onClick = () => {
    setNote(null);
    if (run) {
      analytics.openAnalytics({ runIds: [run.runId], tab: 'jobs' });
      return;
    }
    setBusy(true);
    analyze()
      .then((runId) => analytics.openAnalytics({ runIds: [runId], tab: 'jobs' }))
      .catch((err: unknown) => {
        const error = toApiError(err);
        setNote({ text: error.message, error: error.code !== 'validation' });
      })
      .finally(() => setBusy(false));
  };
  const jobs = run ? `${run.jobs} job${run.jobs === 1 ? '' : 's'}` : '';
  return (
    <>
      <button
        type="button"
        className="analyze-button"
        disabled={busy}
        title={
          run
            ? `Open these ${jobs} in Analytics: ranking, skill gaps and requirements`
            : 'Add the jobs listed in this answer to Analytics: ranking, skill gaps and requirements'
        }
        onClick={onClick}
      >
        <ChartIcon className="analyze-button__icon" />
        {busy ? 'Reading jobs…' : run ? `Analyze ${jobs}` : 'Analyze jobs'}
      </button>
      {note && (
        <p className={note.error ? 'analyze-button__note analyze-button__note--error' : 'analyze-button__note'} role="status">
          <LinkedText text={note.text} />
        </p>
      )}
    </>
  );
}
