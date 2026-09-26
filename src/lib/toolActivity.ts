import type { ToolActivity } from '../services/chatService';

/** Adds or replaces (by call id) one tool activity. */
export function withActivity(list: readonly ToolActivity[], activity: ToolActivity): ToolActivity[] {
  const index = list.findIndex((a) => a.id === activity.id);
  if (index < 0) return [...list, activity];
  return list.map((a, i) => (i === index ? activity : a));
}
