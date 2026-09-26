import {
  commands,
  type ApprovalDecision,
  type Conversation,
  type ConversationDetail,
  type Message,
  type ModelRef,
  type SendMessageInput,
  type SendMessageResult,
} from '../generated/bindings';
import { callBackend } from './ipc';

export type {
  ApprovalDecision,
  ChatEvent,
  Conversation,
  ConversationDetail,
  Message,
  MessageStatus,
  ToolActivity,
  ToolStatus,
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

/** Stores the agents and MCP servers selected in a conversation. */
export function setConversationSelections(
  id: number,
  agentIds: string[],
  mcpServerIds: number[],
): Promise<Conversation> {
  return callBackend(() => commands.setConversationSelections(id, agentIds, mcpServerIds));
}

/** Answers a tool call that waits for the user's approval. */
export function respondToolApproval(messageId: number, callId: string, decision: ApprovalDecision): Promise<null> {
  return callBackend(() => commands.respondToolApproval(messageId, callId, decision));
}
