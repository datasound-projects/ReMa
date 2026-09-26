import { useCallback, useMemo, useState } from 'react';

import { AnalyticsProvider } from '../components/analytics/AnalyticsProvider';
import { BrowserProvider } from '../components/browser/BrowserProvider';
import { ConversationList } from '../components/chat/ConversationList';
import { PlusIcon } from '../components/icons';
import { AppShell } from '../components/layout/AppShell';
import { LaunchIntro } from '../components/layout/LaunchIntro';
import { Sidebar } from '../components/layout/Sidebar';
import { dataOr } from '../hooks/useAsyncData';
import { useConversations } from '../hooks/useConversations';
import { useSidebar } from '../hooks/useSidebar';
import { AgentsPage } from '../pages/AgentsPage';
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
  // "Profile" returns to the part of the Profile that was open last
  // (Documents & Credentials the first time).
  const [lastProfile, setLastProfile] = useState<View & { page: 'profile' }>({
    page: 'profile',
    section: 'documents',
  });
  const conversations = dataOr(useConversations().state, []);
  const sidebar = useSidebar();

  const navigate = useCallback((next: View) => {
    setView(next);
    if (next.page === 'chat') setLastConversationId(next.conversationId);
    if (next.page === 'profile') setLastProfile(next);
  }, []);
  const navigation = useMemo(() => ({ view, navigate }), [view, navigate]);

  const selectPage = (page: PageId) => {
    if (page === 'chat') navigate({ page: 'chat', conversationId: lastConversationId });
    else if (page === 'profile') navigate(lastProfile);
    else navigate({ page });
  };

  const activeConversationId = view.page === 'chat' ? view.conversationId : null;

  return (
    <NavigationContext value={navigation}>
      <BrowserProvider>
      <AnalyticsProvider>
      <AppShell
        sidebarCollapsed={sidebar.collapsed}
        onToggleSidebar={sidebar.toggle}
        sidebar={
          <Sidebar
            items={MAIN_NAV}
            footerItems={FOOTER_NAV}
            activeId={view.page}
            onSelect={selectPage}
            collapsed={sidebar.collapsed}
            rail={
              <button
                type="button"
                className="sidebar__item"
                title="New chat"
                onClick={() => navigate({ page: 'chat', conversationId: null })}
              >
                <PlusIcon className="sidebar__icon" />
                <span className="sidebar__label">New chat</span>
              </button>
            }
          >
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
        {view.page === 'chat' && (
          <ChatPage conversationId={view.conversationId} initialAgentIds={view.agentIds} />
        )}
        {view.page === 'agents' && <AgentsPage />}
        {view.page === 'tasks' && <ScheduledTasksPage />}
        {view.page === 'profile' && (
          <ProfilePage section={view.section ?? 'documents'} portfolioId={view.portfolioId ?? null} />
        )}
        {view.page === 'settings' && <SettingsPage focus={view.focus} />}
      </AppShell>
      {/* Above the app, which loads underneath; plays once per launch. */}
      <LaunchIntro />
      </AnalyticsProvider>
      </BrowserProvider>
    </NavigationContext>
  );
}
