import type { ComponentType } from 'react';

import type { IconProps } from '../icons';

export interface SidebarItem<Id extends string> {
  id: Id;
  label: string;
  icon: ComponentType<IconProps>;
}

interface SidebarProps<Id extends string> {
  items: readonly SidebarItem<Id>[];
  activeId: Id;
  onSelect: (id: Id) => void;
}

export function Sidebar<Id extends string>({ items, activeId, onSelect }: SidebarProps<Id>) {
  return (
    <aside className="sidebar">
      <nav className="sidebar__nav" aria-label="Main">
        {items.map(({ id, label, icon: Icon }) => {
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
        })}
      </nav>
    </aside>
  );
}
