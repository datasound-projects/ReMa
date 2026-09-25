import { useState } from 'react';

import { AppShell } from '../components/layout/AppShell';
import { Sidebar } from '../components/layout/Sidebar';
import { DEFAULT_PAGE_ID, PAGES, type PageId } from './pages';

export function App() {
  const [activePageId, setActivePageId] = useState<PageId>(DEFAULT_PAGE_ID);
  const ActivePage = (PAGES.find((page) => page.id === activePageId) ?? PAGES[0]).component;

  return (
    <AppShell sidebar={<Sidebar items={PAGES} activeId={activePageId} onSelect={setActivePageId} />}>
      <ActivePage />
    </AppShell>
  );
}
