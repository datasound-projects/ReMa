// @vitest-environment jsdom
import '../test/dom';

import { act, renderHook, waitFor } from '@testing-library/react';
import { useState } from 'react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { ChatEvent, Message } from '../services/chatService';
import { useChat } from './useChat';

const chat = vi.hoisted(() => ({
  getConversation: vi.fn(),
  retryMessage: vi.fn(),
  sendMessage: vi.fn(),
  stopGeneration: vi.fn(),
}));

const bus = vi.hoisted(() => {
  const handlers = new Set<(payload: unknown) => void>();
  return {
    handlers,
    emit: (payload: unknown) => handlers.forEach((h) => h(payload)),
  };
});

vi.mock('../services/chatService', () => chat);
vi.mock('../services/events', () => ({
  backendEvents: { chatEvent: {} },
  subscribe: (_event: unknown, handler: (payload: unknown) => void) => {
    bus.handlers.add(handler);
    return () => bus.handlers.delete(handler);
  },
}));

const model = { providerId: 'openai', modelId: 'gpt-5.5' };

const message = (id: number, conversationId: number, overrides: Partial<Message> = {}): Message => ({
  id,
  conversationId,
  role: 'assistant',
  content: '',
  status: 'streaming',
  error: null,
  model,
  activity: [],
  createdAt: 0,
  ...overrides,
});

const conversation = (id: number) => ({
  id,
  title: 'Chat',
  model,
  profileContext: false,
  agentIds: [],
  mcpServerIds: [],
  createdAt: 0,
  updatedAt: 0,
});

const emit = (event: ChatEvent) => act(() => bus.emit(event));

beforeEach(() => {
  vi.clearAllMocks();
  bus.handlers.clear();
});

describe('useChat', () => {
  it('follows its own stream while another conversation streams', async () => {
    chat.getConversation.mockResolvedValue({ conversation: conversation(1), messages: [message(10, 1)] });
    const { result } = renderHook(() => useChat(1, () => {}));
    await waitFor(() => expect(result.current.messages).toHaveLength(1));

    // A reply still streaming in conversation 2 (the user switched away).
    emit({ type: 'delta', conversationId: 2, messageId: 20, text: 'from chat 2' });
    emit({ type: 'finished', message: message(20, 2, { content: 'from chat 2', status: 'complete' }) });
    emit({ type: 'delta', conversationId: 1, messageId: 10, text: 'Hello' });
    emit({ type: 'finished', message: message(10, 1, { content: 'Hello', status: 'complete' }) });

    expect(result.current.messages).toEqual([message(10, 1, { content: 'Hello', status: 'complete' })]);
    expect(result.current.streamingMessage).toBeNull();
  });

  it('keeps what streamed before a new chat learned its id', async () => {
    // Like the chat page: a new chat takes the id `send` created.
    const { result } = renderHook(() => {
      const [id, setId] = useState<number | null>(null);
      return useChat(id, setId);
    });
    let answer!: (value: unknown) => void;
    chat.sendMessage.mockReturnValue(new Promise((resolve) => (answer = resolve)));
    let sent!: Promise<void>;
    act(() => {
      sent = result.current.send('Hi', model, false, { agentIds: [], mcpServerIds: [] });
    });

    emit({ type: 'delta', conversationId: 7, messageId: 71, text: 'Early text' });
    answer({
      conversation: conversation(7),
      userMessage: message(70, 7, { role: 'user', content: 'Hi', status: 'complete' }),
      assistantMessage: message(71, 7),
    });
    await act(() => sent);

    expect(result.current.messages.find((m) => m.id === 71)?.content).toBe('Early text');
  });
});
