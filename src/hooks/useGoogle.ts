import { backendEvents } from '../services/events';
import { getGoogleStatus } from '../services/googleService';
import { useAsyncData } from './useAsyncData';
import { useBackendEvent } from './useBackendEvent';

export function useGoogleStatus() {
  const data = useAsyncData(getGoogleStatus);
  useBackendEvent(backendEvents.googleChanged, data.refresh);
  return data;
}
