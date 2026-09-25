import { useCallback, useEffect, useRef, useState } from 'react';

import { useNavigation } from '../app/navigation';
import { Composer } from '../components/chat/Composer';
import { MessageList } from '../components/chat/MessageList';
import { TaskDialog } from '../components/tasks/TaskDialog';
import { dataOr } from '../hooks/useAsyncData';
import { useChat } from '../hooks/useChat';
import { useModelCatalog } from '../hooks/useModelCatalog';
import { useSystemTimezone } from '../hooks/useTasks';
import { formatDateTime } from '../lib/format';
import { sameModel, type ModelRef } from '../services/providerService';
import type { ScheduledTask } from '../services/taskService';

interface ChatPageProps {
  conversationId: number | null;
}

/** Sticks to the bottom while the user is near it. */
const STICK_THRESHOLD = 80;

export function ChatPage({ conversationId }: ChatPageProps) {
  const { navigate } = useNavigation();
  const onCreated = useCallback(
    (id: number) => navigate({ page: 'chat', conversationId: id }),
    [navigate],
  );
  const chat = useChat(conversationId, onCreated);
  const catalog = dataOr(useModelCatalog().state, null);
  const timezone = dataOr(useSystemTimezone().state, 'UTC');

  // Model: the user's pick for this conversation → the model it last used
  // → the default model → the first available one.
  const [picked, setPicked] = useState<{ conversationId: number | null; model: ModelRef } | null>(null);
  const available = (m: ModelRef | null | undefined): m is ModelRef =>
    !!m && !!catalog?.models.some((o) => sameModel(o.model, m));
  const lastUsed = [...chat.messages].reverse().find((m) => m.model)?.model;
  const model =
    [
      picked?.conversationId === conversationId ? picked.model : null,
      lastUsed,
      catalog?.defaultModel,
      catalog?.models[0]?.model,
    ].find(available) ?? null;

  const [schedulePrompt, setSchedulePrompt] = useState<string | null>(null);
  const [scheduled, setScheduled] = useState<ScheduledTask | null>(null);
  const [clearSignal, setClearSignal] = useState(0);

  const scrollRef = useRef<HTMLDivElement>(null);
  const stick = useRef(true);
  useEffect(() => {
    stick.current = true;
  }, [conversationId]);
  useEffect(() => {
    const el = scrollRef.current;
    if (el && stick.current) el.scrollTop = el.scrollHeight;
  }, [chat.messages]);

  // The confirmation hides itself after a few seconds.
  useEffect(() => {
    if (!scheduled) return;
    const timer = window.setTimeout(() => setScheduled(null), 8000);
    return () => window.clearTimeout(timer);
  }, [scheduled]);

  const empty = conversationId === null && chat.messages.length === 0;

  const composer = (
    <Composer
      catalog={catalog}
      model={model}
      onModelChange={(m) => setPicked({ conversationId, model: m })}
      streaming={!!chat.streamingMessage}
      onSend={(text) => (model ? chat.send(text, model) : Promise.resolve())}
      onStop={() => {
        if (chat.streamingMessage) void chat.stop(chat.streamingMessage.id);
      }}
      onSchedule={setSchedulePrompt}
      clearSignal={clearSignal}
      autoFocus
    />
  );

  return (
    <div className={empty ? 'chat chat--empty' : 'chat'}>
      {empty ? (
        <div className="chat__welcome">
          <h1 className="chat__greeting">What can ReMa help with?</h1>
          {composer}
        </div>
      ) : (
        <>
          <div
            className="chat__scroll"
            ref={scrollRef}
            onScroll={(event) => {
              const el = event.currentTarget;
              stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < STICK_THRESHOLD;
            }}
          >
            {chat.loadError ? (
              <p className="chat__notice" role="alert">
                {chat.loadError.message}
              </p>
            ) : (
              <MessageList
                messages={chat.messages}
                catalog={catalog}
                onRetry={(message) => model && void chat.retry(message.id, model)}
              />
            )}
          </div>
          <div className="chat__footer">{composer}</div>
        </>
      )}

      {scheduled && (
        <div className="toast" role="status">
          <span>
            Scheduled “{scheduled.name}”
            {scheduled.nextRunAt !== null && ` · next run ${formatDateTime(scheduled.nextRunAt)}`}
          </span>
          <button type="button" className="toast__action" onClick={() => navigate({ page: 'tasks' })}>
            View
          </button>
          <button type="button" className="toast__action" onClick={() => setScheduled(null)}>
            Dismiss
          </button>
        </div>
      )}

      {schedulePrompt !== null && (
        <TaskDialog
          catalog={catalog}
          timezone={timezone}
          initialPrompt={schedulePrompt}
          initialModel={model}
          onClose={() => setSchedulePrompt(null)}
          onSaved={(task) => {
            setSchedulePrompt(null);
            setScheduled(task);
            setClearSignal((n) => n + 1);
          }}
        />
      )}
    </div>
  );
}
