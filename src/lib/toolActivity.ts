import type { ToolActivity } from '../services/chatService';

/** Adds or replaces (by call id) one tool activity. */
export function withActivity(list: readonly ToolActivity[], activity: ToolActivity): ToolActivity[] {
  const index = list.findIndex((a) => a.id === activity.id);
  if (index < 0) return [...list, activity];
  return list.map((a, i) => (i === index ? activity : a));
}

/** Web searches and pages the model's provider ran (not MCP tools). */
export const isWebActivity = (a: ToolActivity) => a.kind === 'web_search' || a.kind === 'web_page';
