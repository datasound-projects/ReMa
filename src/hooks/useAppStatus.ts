import { getAppStatus } from '../services/systemService';
import { useAsyncData } from './useAsyncData';

/** Backend status, loaded on mount. */
export function useAppStatus() {
  return useAsyncData(getAppStatus);
}
