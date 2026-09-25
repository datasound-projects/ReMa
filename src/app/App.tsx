import { useCallback, useMemo, useState } from 'react';

import { BrowserProvider } from '../components/browser/BrowserProvider';
import { ConversationList } from '../components/chat/ConversationList';
import { AppShell } from '../components/layout/AppShell';
import { Sidebar } from '../components/layout/Sidebar';
import { dataOr } from '../hooks/useAsyncData';
import { useConversations } from '../hooks/useConversations';
import { ChatPage } from '../pages/ChatPage';
import { ProfilePage } from '../pages/ProfilePage';
import { ScheduledTasksPage } from '../pages/ScheduledTasksPage';
import { SettingsPage } from '../pages/SettingsPage';
import { deleteConversation } from '../services/chatService';
import { NavigationContext, type PageId, type View } from './navigation';
import { FOOTER_NAV, MAIN_NAV } from './pages';

export function App() {
  const [view, setView] = useState<View>({ page: 'chat', conversationId: null });
  // "Chat" in the sidebar returns to the conversation that was open last.
  const [lastConversationId, setLastConversationId] = useState<number | null>(null);
  const conversations = dataOr(useConversations().state, []);

  const navigate = useCallback((next: View) => {
    setView(next);
    if (next.page === 'chat') setLastConversationId(next.conversationId);
  }, []);
  const navigation = useMemo(() => ({ view, navigate }), [view, navigate]);

  const selectPage = (page: PageId) => {
    if (page === 'chat') navigate({ page: 'chat', conversationId: lastConversationId });
    else navigate({ page });
  };

  const activeConversationId = view.page === 'chat' ? view.conversationId : null;

  return (
    <NavigationContext value={navigation}>
      <BrowserProvider>
      <AppShell
        sidebar={
          <Sidebar items={MAIN_NAV} footerItems={FOOTER_NAV} activeId={view.page} onSelect={selectPage}>
            <ConversationList
              conversations={conversations}
              activeId={activeConversationId}
              onOpen={(id) => navigate({ page: 'chat', conversationId: id })}
              onNew={() => navigate({ page: 'chat', conversationId: null })}
              onDelete={(conversation) => {
                void deleteConversation(conversation.id).catch(() => {});
                if (conversation.id === activeConversationId) {
                  navigate({ page: 'chat', conversationId: null });
                } else if (conversation.id === lastConversationId) {
                  setLastConversationId(null);
                }
              }}
            />
          </Sidebar>
        }
      >
        {view.page === 'chat' && <ChatPage conversationId={view.conversationId} />}
        {view.page === 'tasks' && <ScheduledTasksPage />}
        {view.page === 'profile' && <ProfilePage />}
        {view.page === 'settings' && <SettingsPage />}
      </AppShell>
      </BrowserProvider>
    </NavigationContext>
  );
}
