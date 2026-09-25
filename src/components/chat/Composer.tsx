import { useEffect, useRef, useState, type KeyboardEvent } from 'react';

import { toApiError } from '../../services/ipc';
import type { ModelCatalog, ModelRef } from '../../services/providerService';
import { ArrowUpIcon, ClockIcon, StopIcon } from '../icons';
import { ModelSelector } from './ModelSelector';

interface ComposerProps {
  catalog: ModelCatalog | null;
  model: ModelRef | null;
  onModelChange: (model: ModelRef) => void;
  /** The message currently streaming, if any (shows Stop instead of Send). */
  streaming: boolean;
  onSend: (text: string) => Promise<void>;
  onStop: () => void;
  /** Opens scheduling for the current prompt. */
  onSchedule: (text: string) => void;
  /** Clears the text after a successful schedule. */
  clearSignal?: number;
  autoFocus?: boolean;
}

const MAX_HEIGHT = 240;

/** The prompt box: send now, or schedule the same prompt. */
export function Composer({
  catalog,
  model,
  onModelChange,
  streaming,
  onSend,
  onStop,
  onSchedule,
  clearSignal,
  autoFocus,
}: ComposerProps) {
  const [text, setText] = useState('');
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  // Grow with the content up to MAX_HEIGHT.
  useEffect(() => {
    const el = textareaRef.current;
    if (!el) return;
    el.style.height = 'auto';
    el.style.height = `${Math.min(el.scrollHeight, MAX_HEIGHT)}px`;
  }, [text]);

  // Clear after the prompt was scheduled (adjusting state during render).
  const [seenClear, setSeenClear] = useState(clearSignal);
  if (clearSignal !== seenClear) {
    setSeenClear(clearSignal);
    setText('');
  }

  const hasText = text.trim().length > 0;
  const canSend = hasText && !!model && !streaming && !sending;

  const send = async () => {
    if (!canSend) return;
    setSending(true);
    setError(null);
    try {
      await onSend(text);
      setText('');
    } catch (err) {
      setError(toApiError(err).message);
    } finally {
      setSending(false);
      textareaRef.current?.focus();
    }
  };

  const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key === 'Enter' && !event.shiftKey && !event.nativeEvent.isComposing) {
      event.preventDefault();
      void send();
    }
  };

  return (
    <div className="composer-wrap">
      {error && (
        <p className="composer__error" role="alert">
          {error}
        </p>
      )}
      <div className="composer">
        <textarea
          ref={textareaRef}
          className="composer__input"
          rows={1}
          value={text}
          placeholder={model ? 'Ask anything…' : 'Connect a model in Settings to start chatting'}
          aria-label="Message"
          autoFocus={autoFocus}
          onChange={(event) => setText(event.target.value)}
          onKeyDown={onKeyDown}
        />
        <div className="composer__bar">
          <ModelSelector catalog={catalog} value={model} onChange={onModelChange} />
          <div className="composer__actions">
            <button
              type="button"
              className="button button--ghost"
              disabled={!hasText || !model}
              onClick={() => onSchedule(text)}
            >
              <ClockIcon className="button__icon" />
              Schedule
            </button>
            {streaming ? (
              <button
                type="button"
                className="send-button send-button--stop"
                aria-label="Stop generating"
                title="Stop generating"
                onClick={onStop}
              >
                <StopIcon />
              </button>
            ) : (
              <button
                type="button"
                className="send-button"
                aria-label="Send"
                title="Send (Enter)"
                disabled={!canSend}
                onClick={() => void send()}
              >
                <ArrowUpIcon />
              </button>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
