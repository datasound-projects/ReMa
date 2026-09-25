import { backendEvents } from '../services/events';
import { getProviderSettings } from '../services/providerService';
import { useAsyncData } from './useAsyncData';
import { useBackendEvent } from './useBackendEvent';

export function useProviderSettings() {
  const data = useAsyncData(getProviderSettings);
  useBackendEvent(backendEvents.providersChanged, data.refresh);
  return data;
}
