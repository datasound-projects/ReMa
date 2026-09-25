import { backendEvents } from '../services/events';
import { getModelCatalog } from '../services/providerService';
import { useAsyncData } from './useAsyncData';
import { useBackendEvent } from './useBackendEvent';

/** Models the user can choose, updated when providers change. */
export function useModelCatalog() {
  const data = useAsyncData(getModelCatalog);
  useBackendEvent(backendEvents.providersChanged, data.refresh);
  return data;
}
