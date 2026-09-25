import { useCallback, useEffect, useRef, useState } from 'react';

import { useNavigation } from '../app/navigation';
import { Composer } from '../components/chat/Composer';
import { BriefcaseIcon, ChartIcon, SearchIcon, SparkleIcon } from '../components/icons';
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

/** Starting points on an empty chat; picking one only fills the composer. */
const SUGGESTIONS = [
  { icon: SearchIcon, text: 'Find senior AI engineering jobs in Vienna' },
  { icon: ChartIcon, text: 'Which skills do data engineering roles ask for most?' },
  { icon: SparkleIcon, text: 'Draft a short cover letter for a data engineer role' },
  { icon: BriefcaseIcon, text: 'Help me prepare for a technical interview' },
];

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

  // Profile context: the user's choice for this chat, else what the
  // conversation used last. New chats start with it off.
  const [profileChoice, setProfileChoice] = useState<{ conversationId: number | null; on: boolean } | null>(
    null,
  );
  const profileOn =
    profileChoice?.conversationId === conversationId ? profileChoice.on : chat.profileContext;

  const [schedulePrompt, setSchedulePrompt] = useState<string | null>(null);
  const [scheduled, setScheduled] = useState<ScheduledTask | null>(null);
  const [clearSignal, setClearSignal] = useState(0);
  const [draft, setDraft] = useState<{ text: string; seq: number } | undefined>(undefined);

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
      profileOn={profileOn}
      onProfileChange={(on) => setProfileChoice({ conversationId, on })}
      streaming={!!chat.streamingMessage}
      onSend={(text) => (model ? chat.send(text, model, profileOn) : Promise.resolve())}
      onStop={() => {
        if (chat.streamingMessage) void chat.stop(chat.streamingMessage.id);
      }}
      onSchedule={setSchedulePrompt}
      clearSignal={clearSignal}
      draft={draft}
      autoFocus
    />
  );

  return (
    <div className={empty ? 'chat chat--empty' : 'chat'}>
      {empty ? (
        <div className="chat__welcome">
          <div className="chat__heading">
            <h1 className="chat__greeting">What can ReMa help with?</h1>
            <p className="chat__tagline">Search roles, compare them with your Profile, and plan your next step.</p>
          </div>
          {composer}
          {model && (
            <div className="suggestions" aria-label="Suggestions">
              {SUGGESTIONS.map(({ icon: Icon, text }) => (
                <button
                  key={text}
                  type="button"
                  className="suggestion"
                  onClick={() => setDraft((d) => ({ text, seq: (d?.seq ?? 0) + 1 }))}
                >
                  <Icon aria-hidden="true" />
                  {text}
                </button>
              ))}
            </div>
          )}
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
          initialUseProfile={profileOn}
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
