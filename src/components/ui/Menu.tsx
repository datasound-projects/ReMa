import {
  useEffect,
  useId,
  useRef,
  useState,
  type CSSProperties,
  type MouseEvent,
  type ReactNode,
} from 'react';

export interface MenuItem {
  label: string;
  onSelect: () => void;
  danger?: boolean;
  disabled?: boolean;
}

interface MenuProps {
  /** Renders the trigger button; spread `props` onto it. */
  trigger: (props: {
    onClick: (event: MouseEvent<HTMLElement>) => void;
    'aria-expanded': boolean;
    'aria-haspopup': 'menu';
    'aria-controls': string;
  }) => ReactNode;
  /** Menu content: a list of items, or custom content for pickers. */
  items?: MenuItem[];
  children?: (close: () => void) => ReactNode;
  align?: 'start' | 'end';
  /** Opens above the trigger (e.g. inside the composer). */
  placement?: 'below' | 'above';
}

/**
 * A small popover menu. Closes on outside click, Escape, scrolling, or
 * selection. The popover is positioned against the viewport so scrolling
 * or clipped containers (tables, the sidebar) never cut it off.
 */
export function Menu({ trigger, items, children, align = 'end', placement = 'below' }: MenuProps) {
  const [position, setPosition] = useState<CSSProperties | null>(null);
  const open = position !== null;
  const rootRef = useRef<HTMLDivElement>(null);
  const id = useId();

  useEffect(() => {
    if (!open) return;
    const close = () => setPosition(null);
    const onPointer = (event: globalThis.MouseEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) close();
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') close();
    };
    const onScroll = (event: Event) => {
      // Scrolling inside the menu itself is fine.
      if (!rootRef.current?.contains(event.target as Node)) close();
    };
    document.addEventListener('mousedown', onPointer);
    document.addEventListener('keydown', onKey);
    window.addEventListener('scroll', onScroll, true);
    window.addEventListener('resize', close);
    return () => {
      document.removeEventListener('mousedown', onPointer);
      document.removeEventListener('keydown', onKey);
      window.removeEventListener('scroll', onScroll, true);
      window.removeEventListener('resize', close);
    };
  }, [open]);

  const toggle = (event: MouseEvent<HTMLElement>) => {
    if (open) {
      setPosition(null);
      return;
    }
    const rect = event.currentTarget.getBoundingClientRect();
    setPosition({
      ...(placement === 'below'
        ? { top: rect.bottom + 4 }
        : { bottom: window.innerHeight - rect.top + 6 }),
      ...(align === 'end'
        ? { right: window.innerWidth - rect.right }
        : { left: rect.left }),
    });
  };

  const close = () => setPosition(null);

  return (
    <div className="menu" ref={rootRef} onClick={(event) => event.stopPropagation()}>
      {trigger({
        onClick: toggle,
        'aria-expanded': open,
        'aria-haspopup': 'menu',
        'aria-controls': id,
      })}
      {open && (
        <div id={id} role="menu" className="menu__popover" style={position ?? undefined}>
          {items?.map((item) => (
            <button
              key={item.label}
              type="button"
              role="menuitem"
              disabled={item.disabled}
              className={item.danger ? 'menu__item menu__item--danger' : 'menu__item'}
              onClick={() => {
                close();
                item.onSelect();
              }}
            >
              {item.label}
            </button>
          ))}
          {children?.(close)}
        </div>
      )}
    </div>
  );
}
