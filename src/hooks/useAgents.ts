import { backendEvents } from '../services/events';
import { listAgents } from '../services/agentService';
import { useAsyncData } from './useAsyncData';
import { useBackendEvent } from './useBackendEvent';

export function useAgents() {
  const data = useAsyncData(listAgents);
  useBackendEvent(backendEvents.agentsChanged, data.refresh);
  return data;
}
