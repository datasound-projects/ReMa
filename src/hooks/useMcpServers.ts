import { backendEvents } from '../services/events';
import { listMcpServers } from '../services/mcpService';
import { useAsyncData } from './useAsyncData';
import { useBackendEvent } from './useBackendEvent';

export function useMcpServers() {
  const data = useAsyncData(listMcpServers);
  useBackendEvent(backendEvents.mcpChanged, data.refresh);
  return data;
}
