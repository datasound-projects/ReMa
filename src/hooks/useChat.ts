import { useCallback, useEffect, useRef, useState } from 'react';

import { EMPTY_SELECTION, type ChatSelection } from '../lib/chatSelection';
import { withActivity } from '../lib/toolActivity';
import {
  getConversation,
  retryMessage,
  sendMessage,
  stopGeneration,
  type ChatEvent,
  type Conversation,
  type Message,
  type ToolActivity,
} from '../services/chatService';
import { backendEvents } from '../services/events';
import { toApiError, type ApiError } from '../services/ipc';
import type { ModelRef } from '../services/providerService';
import { useBackendEvent } from './useBackendEvent';

/** Stream updates that arrived before their message was known locally. */
interface Pending {
  text: string;
  activity: ToolActivity[];
  final?: Message;
}


const selectionOf = (c: Conversation): ChatSelection => ({ agentIds: c.agentIds, mcpServerIds: c.mcpServerIds });

interface ChatState {
  conversationId: number | null;
  /** The conversation shares the user's Profile with the model. */
  profileContext: boolean;
  /** Agents and MCP servers stored for the conversation. */
  selection: ChatSelection;
  messages: Message[];
  loading: boolean;
  error: ApiError | null;
}

const initial = (conversationId: number | null): ChatState => ({
  conversationId,
  profileContext: false,
  selection: EMPTY_SELECTION,
  messages: [],
  loading: conversationId !== null,
  error: null,
});

/**
 * Messages of one conversation, with live streaming.
 *
 * Streaming text arrives as backend events; command responses and events
 * travel separately, so updates for a message that is not in the list yet
 * are buffered and applied when it appears. The final `finished` event
 * always carries the complete message.
 */
export function useChat(conversationId: number | null, onCreated: (id: number) => void) {
  const [chat, setChat] = useState<ChatState>(() => initial(conversationId));
  // Conversation whose messages are already present (loaded or just created).
  const loadedId = useRef<number | null>(null);
  const pending = useRef(new Map<number, Pending>());

  // Switching conversations resets the view during render (no stale flash).
  // A conversation created by `send` already carries its id, so it is kept.
  if (chat.conversationId !== conversationId) {
    setChat(initial(conversationId));
  }

  useEffect(() => {
    if (conversationId === null || loadedId.current === conversationId) return;
    let active = true;
    pending.current.clear();
    getConversation(conversationId)
      .then((detail) => {
        if (!active) return;
        loadedId.current = conversationId;
        pending.current.clear();
        setChat({
          conversationId,
          profileContext: detail.conversation.profileContext,
          selection: selectionOf(detail.conversation),
          messages: detail.messages,
          loading: false,
          error: null,
        });
      })
      .catch((error: unknown) => {
        if (active) {
          setChat((c) => ({ ...c, loading: false, error: toApiError(error) }));
        }
      });
    return () => {
      active = false;
    };
  }, [conversationId]);

  const updateMessages = (update: (messages: Message[]) => Message[]) =>
    setChat((c) => ({ ...c, messages: update(c.messages) }));

  const applyPending = useCallback((message: Message): Message => {
    const update = pending.current.get(message.id);
    if (!update) return message;
    pending.current.delete(message.id);
    return (
      update.final ?? {
        ...message,
        content: message.content + update.text,
        activity: update.activity.reduce(withActivity, message.activity),
      }
    );
  }, []);

  useBackendEvent(backendEvents.chatEvent, (event: ChatEvent) => {
    const id = event.type === 'finished' ? event.message.id : event.messageId;
    setChat((c) => {
      if (!c.messages.some((m) => m.id === id)) {
        const entry = pending.current.get(id) ?? { text: '', activity: [] };
        if (event.type === 'delta') entry.text += event.text;
        else if (event.type === 'activity') entry.activity.push(event.activity);
        else entry.final = event.message;
        pending.current.set(id, entry);
        return c;
      }
      return {
        ...c,
        messages: c.messages.map((m) => {
          if (m.id !== id) return m;
          switch (event.type) {
            case 'delta':
              return { ...m, content: m.content + event.text };
            case 'activity':
              return { ...m, activity: withActivity(m.activity, event.activity) };
            case 'finished':
              return event.message;
          }
        }),
      };
    });
  });

  const send = useCallback(
    async (content: string, model: ModelRef, useProfile: boolean, selection: ChatSelection) => {
      const result = await sendMessage({
        conversationId,
        content,
        model,
        useProfile,
        agentIds: selection.agentIds,
        mcpServerIds: selection.mcpServerIds,
      });
      const added = [result.userMessage, applyPending(result.assistantMessage)];
      loadedId.current = result.conversation.id;
      if (conversationId === null) {
        setChat({
          conversationId: result.conversation.id,
          profileContext: result.conversation.profileContext,
          selection: selectionOf(result.conversation),
          messages: added,
          loading: false,
          error: null,
        });
        onCreated(result.conversation.id);
      } else {
        setChat((c) => ({
          ...c,
          profileContext: result.conversation.profileContext,
          selection: selectionOf(result.conversation),
          messages: [...c.messages, ...added],
        }));
      }
    },
    [conversationId, onCreated, applyPending],
  );

  const stop = useCallback((messageId: number) => stopGeneration(messageId), []);

  const retry = useCallback(async (messageId: number, model: ModelRef) => {
    // Clear the old text first so new stream text is not appended to it.
    pending.current.delete(messageId);
    updateMessages((messages) =>
      messages.map((m) =>
        m.id === messageId ? { ...m, content: '', status: 'streaming', error: null, model, activity: [] } : m,
      ),
    );
    try {
      await retryMessage(messageId, model);
    } catch (error) {
      const apiError = toApiError(error);
      updateMessages((messages) =>
        messages.map((m) =>
          m.id === messageId ? { ...m, status: 'error', error: apiError.message } : m,
        ),
      );
    }
  }, []);

  const messages = chat.conversationId === conversationId ? chat.messages : [];
  const streamingMessage = messages.find((m) => m.status === 'streaming') ?? null;

  return {
    messages,
    loading: chat.loading,
    loadError: chat.error,
    profileContext: chat.conversationId === conversationId && chat.profileContext,
    selection: chat.conversationId === conversationId ? chat.selection : EMPTY_SELECTION,
    send,
    stop,
    retry,
    streamingMessage,
  };
}
