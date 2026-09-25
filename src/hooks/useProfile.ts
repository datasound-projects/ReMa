import { backendEvents } from '../services/events';
import { getProfile } from '../services/profileService';
import { useAsyncData } from './useAsyncData';
import { useBackendEvent } from './useBackendEvent';

export function useProfile() {
  const data = useAsyncData(getProfile);
  useBackendEvent(backendEvents.profileChanged, data.refresh);
  return data;
}
