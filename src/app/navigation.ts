import { createContext, useContext } from 'react';

/** The two parts of the Profile page. */
export type ProfileSection = 'documents' | 'custom';

/** What the main area shows. */
export type View =
  | { page: 'chat'; conversationId: number | null; agentIds?: string[] }
  | { page: 'agents' }
  /** `taskId`: the task whose runs are open; `runId`: the run shown (the newest when absent). */
  | { page: 'tasks'; taskId?: number | null; runId?: number | null }
  | { page: 'profile'; section?: ProfileSection }
  /** `portfolioId`: the Portfolio Studio document open in the editor. */
  | { page: 'portfolio'; portfolioId?: number | null }
  /** `applicationId`: the application whose details are open. */
  | { page: 'applications'; applicationId?: number | null }
  /** `focus` scrolls to a section (e.g. MCP from the chat's + menu). */
  | { page: 'settings'; focus?: 'mcp' | 'connectors' };

export type PageId = View['page'];

interface Navigation {
  view: View;
  navigate: (view: View) => void;
}

export const NavigationContext = createContext<Navigation | null>(null);

export function useNavigation(): Navigation {
  const navigation = useContext(NavigationContext);
  if (!navigation) throw new Error('useNavigation must be used inside NavigationContext');
  return navigation;
}
