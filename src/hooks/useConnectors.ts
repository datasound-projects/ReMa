import { backendEvents } from '../services/events';
import { getConnectors } from '../services/connectorService';
import { useAsyncData } from './useAsyncData';
import { useBackendEvent } from './useBackendEvent';

/** The four connectors, kept current as sign-ins and syncs change them. */
export function useConnectors() {
  const data = useAsyncData(getConnectors);
  useBackendEvent(backendEvents.connectorsChanged, data.refresh);
  return data;
}
