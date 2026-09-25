import { ChatIcon, ClockIcon, ProfileIcon, SettingsIcon } from '../components/icons';
import type { SidebarItem } from '../components/layout/Sidebar';
import type { PageId } from './navigation';

/** Main navigation. Adding a page means adding an entry here. */
export const MAIN_NAV: readonly SidebarItem<PageId>[] = [
  { id: 'chat', label: 'Chat', icon: ChatIcon },
  { id: 'tasks', label: 'Scheduled Tasks', icon: ClockIcon },
  { id: 'profile', label: 'Profile', icon: ProfileIcon },
];

/** Pinned to the bottom of the sidebar. */
export const FOOTER_NAV: readonly SidebarItem<PageId>[] = [
  { id: 'settings', label: 'Settings', icon: SettingsIcon },
];
