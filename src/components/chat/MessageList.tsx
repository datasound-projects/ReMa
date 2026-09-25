import { useState } from 'react';

import { modelName } from '../../lib/format';
import type { Message } from '../../services/chatService';
import type { ModelCatalog } from '../../services/providerService';
import { CheckIcon, CopyIcon, RetryIcon } from '../icons';
import { IconButton } from '../ui/IconButton';
import { Markdown } from './Markdown';

interface MessageListProps {
  messages: Message[];
  catalog: ModelCatalog | null;
  onRetry: (message: Message) => void;
}

export function MessageList({ messages, catalog, onRetry }: MessageListProps) {
  const lastId = messages.at(-1)?.id;
  return (
    <div className="thread">
      {messages.map((message) =>
        message.role === 'user' ? (
          <div key={message.id} className="message message--user">
            <div className="message__bubble">{message.content}</div>
          </div>
        ) : (
          <AssistantMessage
            key={message.id}
            message={message}
            catalog={catalog}
            canRetry={message.id === lastId}
            onRetry={() => onRetry(message)}
          />
        ),
      )}
    </div>
  );
}

interface AssistantMessageProps {
  message: Message;
  catalog: ModelCatalog | null;
  canRetry: boolean;
  onRetry: () => void;
}

function AssistantMessage({ message, catalog, canRetry, onRetry }: AssistantMessageProps) {
  const streaming = message.status === 'streaming';
  return (
    <div className="message message--assistant" aria-busy={streaming}>
      {message.content ? (
        <Markdown source={message.content} />
      ) : (
        streaming && (
          <div className="typing" aria-label="Generating">
            <span />
            <span />
            <span />
          </div>
        )
      )}
      {message.status === 'error' && (
        <div className="message__error" role="alert">
          {message.error ?? 'Something went wrong.'}
        </div>
      )}
      {!streaming && (
        <div className="message__actions">
          {message.content && <CopyButton text={message.content} />}
          {canRetry && (
            <IconButton label="Retry" className="icon-button--small" onClick={onRetry}>
              <RetryIcon />
            </IconButton>
          )}
          {message.status === 'stopped' && <span className="message__note">Stopped</span>}
          {message.model && (
            <span className="message__model">{modelName(catalog, message.model)}</span>
          )}
        </div>
      )}
    </div>
  );
}

function CopyButton({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      // Fallback for webviews without the async clipboard API.
      const area = document.createElement('textarea');
      area.value = text;
      document.body.appendChild(area);
      area.select();
      document.execCommand('copy');
      area.remove();
    }
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1500);
  };
  return (
    <IconButton label={copied ? 'Copied' : 'Copy'} className="icon-button--small" onClick={() => void copy()}>
      {copied ? <CheckIcon /> : <CopyIcon />}
    </IconButton>
  );
}
