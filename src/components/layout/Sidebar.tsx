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
  /** Content between the main and footer navigation (e.g. recent chats). */
  children?: ReactNode;
}

function NavItems<Id extends string>({
  items,
  activeId,
  onSelect,
}: Pick<SidebarProps<Id>, 'items' | 'activeId' | 'onSelect'>) {
  return items.map(({ id, label, icon: Icon }) => {
    const active = id === activeId;
    return (
      <button
        key={id}
        type="button"
        className={active ? 'sidebar__item sidebar__item--active' : 'sidebar__item'}
        aria-current={active ? 'page' : undefined}
        onClick={() => onSelect(id)}
      >
        <Icon className="sidebar__icon" />
        <span>{label}</span>
      </button>
    );
  });
}

export function Sidebar<Id extends string>({
  items,
  footerItems,
  activeId,
  onSelect,
  children,
}: SidebarProps<Id>) {
  return (
    <aside className="sidebar">
      <nav className="sidebar__nav" aria-label="Main">
        <NavItems items={items} activeId={activeId} onSelect={onSelect} />
      </nav>
      <div className="sidebar__content">{children}</div>
      <nav className="sidebar__nav sidebar__footer" aria-label="Settings">
        <NavItems items={footerItems} activeId={activeId} onSelect={onSelect} />
      </nav>
    </aside>
  );
}
