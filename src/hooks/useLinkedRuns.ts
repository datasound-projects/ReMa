import { useMemo } from 'react';

import type { LinkedRun } from '../services/analyticsService';
import { useAnalyticsData } from './useAnalyticsData';

/** Search runs made from answers or task runs, by source id (kept up to date). */
export function useLinkedRuns(load: (() => Promise<LinkedRun[]>) | null, key: string): Map<number, LinkedRun> {
  const { data } = useAnalyticsData(load, key);
  return useMemo(() => new Map((data ?? []).map((r) => [r.sourceId, r])), [data]);
}
