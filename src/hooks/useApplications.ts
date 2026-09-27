import { useCallback } from 'react';

import { backendEvents } from '../services/events';
import { getApplication, getApplications, listNotifications } from '../services/applicationService';
import { useAsyncData } from './useAsyncData';
import { useBackendEvent } from './useBackendEvent';

/** The application tracker overview, refreshed as mail is processed. */
export function useApplications() {
  const data = useAsyncData(getApplications);
  useBackendEvent(backendEvents.applicationsChanged, data.refresh);
  return data;
}

/** One application with its timeline, interviews and correspondence. */
export function useApplication(id: number) {
  const load = useCallback(() => getApplication(id), [id]);
  const data = useAsyncData(load);
  useBackendEvent(backendEvents.applicationsChanged, data.refresh);
  return data;
}

export function useNotifications() {
  const data = useAsyncData(listNotifications);
  useBackendEvent(backendEvents.notificationsChanged, data.refresh);
  return data;
}
