import { listConversations } from '../services/chatService';
import { backendEvents } from '../services/events';
import { useAsyncData } from './useAsyncData';
import { useBackendEvent } from './useBackendEvent';

/** Conversation history, kept up to date by backend events. */
export function useConversations() {
  const data = useAsyncData(listConversations);
  useBackendEvent(backendEvents.conversationsChanged, data.refresh);
  return data;
}
