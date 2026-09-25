import { createContext, useContext } from 'react';

/** What the main area shows. */
export type View =
  | { page: 'chat'; conversationId: number | null }
  | { page: 'tasks' }
  | { page: 'settings' };

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
