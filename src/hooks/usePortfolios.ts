import { backendEvents } from '../services/events';
import { listPortfolios } from '../services/portfolioService';
import { useAsyncData } from './useAsyncData';
import { useBackendEvent } from './useBackendEvent';

export function usePortfolios() {
  const data = useAsyncData(listPortfolios);
  useBackendEvent(backendEvents.portfolioChanged, data.refresh);
  return data;
}
