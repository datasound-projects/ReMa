import {
  commands,
  type Conversation,
  type ConversationDetail,
  type Message,
  type ModelRef,
  type SendMessageInput,
  type SendMessageResult,
} from '../generated/bindings';
import { callBackend } from './ipc';

export type {
  ChatEvent,
  Conversation,
  ConversationDetail,
  Message,
  MessageStatus,
} from '../generated/bindings';

export function listConversations(): Promise<Conversation[]> {
  return callBackend(() => commands.listConversations());
}

export function getConversation(id: number): Promise<ConversationDetail> {
  return callBackend(() => commands.getConversation(id));
}

export function sendMessage(input: SendMessageInput): Promise<SendMessageResult> {
  return callBackend(() => commands.sendMessage(input));
}

export function retryMessage(messageId: number, model: ModelRef): Promise<Message> {
  return callBackend(() => commands.retryMessage(messageId, model));
}

export function stopGeneration(messageId: number): Promise<null> {
  return callBackend(() => commands.stopGeneration(messageId));
}

export function deleteConversation(id: number): Promise<null> {
  return callBackend(() => commands.deleteConversation(id));
}
