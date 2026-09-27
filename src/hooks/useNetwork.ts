import { backendEvents } from '../services/events';
import { getNetworkCapabilities } from '../services/networkService';
import { useAsyncData } from './useAsyncData';
import { useBackendEvent } from './useBackendEvent';

/** LinkedIn's and XING's capabilities, current as connections change. */
export function useNetworkCapabilities() {
  const data = useAsyncData(getNetworkCapabilities);
  useBackendEvent(backendEvents.connectorsChanged, data.refresh);
  return data;
}
