import type { Conversation } from '../../services/chatService';
import { MoreIcon, PlusIcon } from '../icons';
import { IconButton } from '../ui/IconButton';
import { Menu } from '../ui/Menu';

interface ConversationListProps {
  conversations: Conversation[];
  activeId: number | null;
  onOpen: (id: number) => void;
  onNew: () => void;
  onDelete: (conversation: Conversation) => void;
}

/** "Recents" in the sidebar. */
export function ConversationList({
  conversations,
  activeId,
  onOpen,
  onNew,
  onDelete,
}: ConversationListProps) {
  return (
    <section className="recents" aria-label="Recent chats">
      <div className="recents__header">
        <span className="recents__label">Recents</span>
        <IconButton label="New chat" className="icon-button--small" onClick={onNew}>
          <PlusIcon />
        </IconButton>
      </div>
      {conversations.length === 0 ? (
        <p className="recents__empty">No chats yet</p>
      ) : (
        <ul className="recents__list">
          {conversations.map((conversation) => (
            <li
              key={conversation.id}
              className={
                conversation.id === activeId ? 'recents__row recents__row--active' : 'recents__row'
              }
            >
              <button
                type="button"
                className="recents__item"
                aria-current={conversation.id === activeId ? 'page' : undefined}
                title={conversation.title}
                onClick={() => onOpen(conversation.id)}
              >
                {conversation.title}
              </button>
              <Menu
                items={[{ label: 'Delete', danger: true, onSelect: () => onDelete(conversation) }]}
                trigger={(props) => (
                  <IconButton label="Chat options" className="recents__more" {...props}>
                    <MoreIcon />
                  </IconButton>
                )}
              />
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
