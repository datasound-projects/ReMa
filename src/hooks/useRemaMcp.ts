import { useCallback } from 'react';

import { backendEvents } from '../services/events';
import { remaMcpStatus } from '../services/remaMcpService';
import { useAsyncData } from './useAsyncData';
import { useBackendEvent } from './useBackendEvent';

/** ReMa MCP's status, refreshed when MCP settings change. */
export function useRemaMcp() {
  const load = useCallback(() => remaMcpStatus(false), []);
  const data = useAsyncData(load);
  useBackendEvent(backendEvents.mcpChanged, data.refresh);
  return data;
}
