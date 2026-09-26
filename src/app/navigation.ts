import { createContext, useContext } from 'react';

/** The three independent parts of the Profile page. */
export type ProfileSection = 'documents' | 'custom' | 'portfolio';

/** What the main area shows. */
export type View =
  | { page: 'chat'; conversationId: number | null; agentIds?: string[] }
  | { page: 'agents' }
  | { page: 'tasks' }
  /** `portfolioId`: the Portfolio Studio document open in the editor. */
  | { page: 'profile'; section?: ProfileSection; portfolioId?: number | null }
  /** `focus` scrolls to a section (e.g. MCP from the chat's + menu). */
  | { page: 'settings'; focus?: 'mcp' };

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
