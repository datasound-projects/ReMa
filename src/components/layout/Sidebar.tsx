import type { ComponentType, ReactNode } from 'react';

import type { IconProps } from '../icons';

export interface SidebarItem<Id extends string> {
  id: Id;
  label: string;
  icon: ComponentType<IconProps>;
}

interface SidebarProps<Id extends string> {
  items: readonly SidebarItem<Id>[];
  footerItems: readonly SidebarItem<Id>[];
  activeId: Id;
  onSelect: (id: Id) => void;
  /** Collapsed to an icon rail: labels hide, `rail` replaces `children`. */
  collapsed?: boolean;
  /** Content between the main and footer navigation (e.g. recent chats). */
  children?: ReactNode;
  /** What the collapsed rail shows instead (e.g. a "New chat" button). */
  rail?: ReactNode;
}

function NavItems<Id extends string>({
  items,
  activeId,
  onSelect,
  collapsed,
}: Pick<SidebarProps<Id>, 'items' | 'activeId' | 'onSelect' | 'collapsed'>) {
  return items.map(({ id, label, icon: Icon }) => {
    const active = id === activeId;
    return (
      <button
        key={id}
        type="button"
        className={active ? 'sidebar__item sidebar__item--active' : 'sidebar__item'}
        aria-current={active ? 'page' : undefined}
        // The rail shows icons only; the label becomes the tooltip.
        title={collapsed ? label : undefined}
        onClick={() => onSelect(id)}
      >
        <Icon className="sidebar__icon" />
        <span className="sidebar__label">{label}</span>
      </button>
    );
  });
}

export function Sidebar<Id extends string>({
  items,
  footerItems,
  activeId,
  onSelect,
  collapsed = false,
  children,
  rail,
}: SidebarProps<Id>) {
  return (
    <aside id="app-sidebar" className={collapsed ? 'sidebar sidebar--collapsed' : 'sidebar'} aria-label="Sidebar">
      <nav className="sidebar__nav" aria-label="Main">
        <NavItems items={items} activeId={activeId} onSelect={onSelect} collapsed={collapsed} />
      </nav>
      <div className="sidebar__content">{collapsed ? <div className="sidebar__rail">{rail}</div> : children}</div>
      <nav className="sidebar__nav sidebar__footer" aria-label="Settings">
        <NavItems items={footerItems} activeId={activeId} onSelect={onSelect} collapsed={collapsed} />
      </nav>
    </aside>
  );
}
