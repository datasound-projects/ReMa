import { useEffect, useRef, useState } from 'react';

import { useCoverBrowser } from '../../app/browser';
import { useNavigation } from '../../app/navigation';
import { useNotifications } from '../../hooks/useApplications';
import { dataOr } from '../../hooks/useAsyncData';
import { formatRelative } from '../../lib/format';
import { clearNotifications, markNotificationsRead, type NotificationItem } from '../../services/applicationService';
import { BellIcon } from '../icons';
import { IconButton } from '../ui/IconButton';

/**
 * Application updates, interviews, conflicts and connector problems. The
 * same notifications are also shown by the operating system.
 */
export function NotificationBell() {
  const notifications = useNotifications();
  const items = dataOr(notifications.state, [] as NotificationItem[]);
  const unread = items.filter((n) => !n.read).length;
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onPointer = (event: PointerEvent) => {
      if (ref.current && !ref.current.contains(event.target as Node)) setOpen(false);
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setOpen(false);
    };
    window.addEventListener('pointerdown', onPointer);
    window.addEventListener('keydown', onKey);
    return () => {
      window.removeEventListener('pointerdown', onPointer);
      window.removeEventListener('keydown', onKey);
    };
  }, [open]);

  const toggle = () => {
    const next = !open;
    setOpen(next);
    if (next && unread > 0) void markNotificationsRead(null).catch(() => {});
  };

  return (
    <div className="notifications" ref={ref}>
      <IconButton
        label={unread > 0 ? `Notifications (${unread} new)` : 'Notifications'}
        aria-expanded={open}
        aria-haspopup="dialog"
        className="icon-button--small notifications__button"
        onClick={toggle}
      >
        <BellIcon />
        {unread > 0 && (
          <span className="notifications__badge" aria-hidden="true">
            {unread > 9 ? '9+' : unread}
          </span>
        )}
      </IconButton>
      {open && <NotificationPanel items={items} onClose={() => setOpen(false)} />}
    </div>
  );
}

function NotificationPanel({ items, onClose }: { items: NotificationItem[]; onClose: () => void }) {
  const { navigate } = useNavigation();
  // Web pages are drawn natively above ReMa; hide the page meanwhile.
  useCoverBrowser();
  return (
    <div className="notifications__panel" role="dialog" aria-label="Notifications">
      <div className="notifications__head">
        <span className="notifications__title">Notifications</span>
        {items.length > 0 && (
          <button type="button" className="link-button" onClick={() => void clearNotifications().catch(() => {})}>
            Clear all
          </button>
        )}
      </div>
      {items.length === 0 ? (
        <p className="notifications__empty">
          Nothing yet. Application updates, interviews and calendar conflicts from your connected mail appear here.
        </p>
      ) : (
        <ul className="notifications__list">
          {items.map((n) => {
            const target =
              n.applicationId !== null
                ? () => {
                    onClose();
                    navigate({ page: 'applications', applicationId: n.applicationId });
                  }
                : n.kind === 'connector'
                  ? () => {
                      onClose();
                      navigate({ page: 'settings', focus: 'connectors' });
                    }
                  : null;
            const body = (
              <>
                <span className="notifications__item-title">{n.title}</span>
                <span className="notifications__item-body">{n.body}</span>
                <span className="notifications__item-time">{formatRelative(n.createdAt)}</span>
              </>
            );
            return (
              <li key={n.id} className={n.read ? 'notifications__item' : 'notifications__item notifications__item--new'}>
                {target ? (
                  <button type="button" className="notifications__item-button" onClick={target}>
                    {body}
                  </button>
                ) : (
                  <div className="notifications__item-button">{body}</div>
                )}
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
