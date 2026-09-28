import { useEffect, useLayoutEffect, useRef, type CSSProperties, type KeyboardEvent } from 'react';

import type { FieldTarget } from '../../../lib/portfolio/fields';
import { htmlToLine, joinLines, lineToHtml, splitLines, type Line } from '../../../lib/portfolio/markup';

interface LineEditorProps {
  value: string;
  onChange: (value: string) => void;
  target: FieldTarget;
  className?: string;
  style?: CSSProperties;
  placeholder?: string;
  /** Blank lines separate paragraphs (cover letter body). */
  paragraphs?: boolean;
  label: string;
  onFocus?: () => void;
}

/**
 * Multi-line text on the canvas with bullets and inline formatting: each
 * line is edited in place; lines starting with "- " show as bullets;
 * **bold**, _italic_ and links render as such and are kept as markup.
 */
export function LineEditor({ value, onChange, target, className, style, placeholder, paragraphs, label, onFocus }: LineEditorProps) {
  const lines = splitLines(value);
  const root = useRef<HTMLDivElement>(null);
  const focusAt = useRef<{ index: number; offset: 'start' | 'end' } | null>(null);
  const setFocusAt = (value: { index: number; offset: 'start' | 'end' } | null) => {
    focusAt.current = value;
  };
  const shown: Line[] = lines.length ? lines : [{ bullet: false, text: '' }];

  const update = (next: Line[]) => onChange(joinLines(next));

  const lineAt = (index: number): HTMLElement | null => root.current?.querySelectorAll<HTMLElement>('[data-line]')[index] ?? null;

  // After a line was added or removed, the caret moves to the right line.
  useLayoutEffect(() => {
    const wanted = focusAt.current;
    if (!wanted) return;
    focusAt.current = null;
    const el = lineAt(wanted.index);
    if (el) {
      el.focus();
      const range = document.createRange();
      range.selectNodeContents(el);
      range.collapse(wanted.offset === 'start');
      const selection = window.getSelection();
      selection?.removeAllRanges();
      selection?.addRange(range);
    }
  });

  const onKeyDown = (event: KeyboardEvent<HTMLElement>, index: number) => {
    const el = event.currentTarget;
    const current = shown[index] ?? { bullet: false, text: '' };
    if (event.key === 'Enter' && !event.shiftKey) {
      event.preventDefault();
      const text = htmlToLine(el);
      const selection = window.getSelection();
      const caret = selection && selection.rangeCount ? caretOffset(el, selection.getRangeAt(0)) : text.length;
      const plainLength = el.textContent?.length ?? 0;
      const atEnd = caret >= plainLength;
      if (current.bullet && text.trim() === '') {
        // An empty bullet ends the list.
        update(shown.map((l, i) => (i === index ? { bullet: false, text: '' } : l)));
        return;
      }
      const next = [...shown];
      if (atEnd) {
        next.splice(index + 1, 0, { bullet: current.bullet, text: '' });
      } else {
        // Split plain text at the caret (formatting on the split line is kept as text).
        const plain = el.textContent ?? '';
        next[index] = { bullet: current.bullet, text: plain.slice(0, caret) };
        next.splice(index + 1, 0, { bullet: current.bullet, text: plain.slice(caret) });
      }
      update(next);
      setFocusAt({ index: index + 1, offset: 'start' });
      return;
    }
    if (event.key === 'Backspace' && (el.textContent ?? '') === '') {
      event.preventDefault();
      if (current.bullet) {
        update(shown.map((l, i) => (i === index ? { ...l, bullet: false } : l)));
        return;
      }
      if (shown.length > 1) {
        update(shown.filter((_, i) => i !== index));
        setFocusAt({ index: Math.max(0, index - 1), offset: 'end' });
      }
      return;
    }
    if (event.key === 'ArrowUp' && index > 0 && isOnFirstLine(el)) {
      event.preventDefault();
      setFocusAt({ index: index - 1, offset: 'end' });
    }
    if (event.key === 'ArrowDown' && index < shown.length - 1 && isOnLastLine(el)) {
      event.preventDefault();
      setFocusAt({ index: index + 1, offset: 'end' });
    }
    if (event.key === 'Escape') el.blur();
  };

  const onInput = (el: HTMLElement, index: number) => {
    const text = htmlToLine(el);
    const line = shown[index];
    if (!line || line.text === text) return;
    update(shown.map((l, i) => (i === index ? { ...l, text } : l)));
  };

  return (
    <div
      ref={root}
      className={`cs-lines${className ? ` ${className}` : ''}${value ? '' : ' cs-lines--empty'}`}
      style={style}
      role="textbox"
      aria-label={label}
      aria-multiline
      data-placeholder={placeholder}
      data-target={JSON.stringify(target)}
      data-rich
      onFocus={onFocus}
    >
      {shown.map((line, index) => (
        <LineView
          key={index}
          line={line}
          index={index}
          paragraph={!!paragraphs && !line.bullet && line.text.trim() !== ''}
          onKeyDown={(e) => onKeyDown(e, index)}
          onInput={(el) => onInput(el, index)}
          onToggleBullet={() => update(shown.map((l, i) => (i === index ? { ...l, bullet: !l.bullet } : l)))}
        />
      ))}
    </div>
  );
}

function LineView({
  line,
  index,
  paragraph,
  onKeyDown,
  onInput,
  onToggleBullet,
}: {
  line: Line;
  index: number;
  paragraph: boolean;
  onKeyDown: (event: KeyboardEvent<HTMLElement>) => void;
  onInput: (el: HTMLElement) => void;
  onToggleBullet: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const html = lineToHtml(line.text);
  useEffect(() => {
    const el = ref.current;
    if (!el || document.activeElement === el) return;
    if (el.innerHTML !== html) el.innerHTML = html;
  }, [html]);
  return (
    <div className={`cs-line${line.bullet ? ' cs-line--bullet' : ''}${paragraph ? ' cs-line--paragraph' : ''}`}>
      {line.bullet && <span className="cs-line__marker" aria-hidden="true" />}
      <div
        ref={ref}
        className="cs-line__text"
        contentEditable
        suppressContentEditableWarning
        data-line={index}
        data-bullet={line.bullet ? 'true' : undefined}
        spellCheck
        onKeyDown={onKeyDown}
        onInput={(e) => onInput(e.currentTarget)}
        onBlur={(e) => onInput(e.currentTarget)}
        onPaste={(e) => {
          e.preventDefault();
          const text = e.clipboardData.getData('text/plain').replace(/[\r\n]+/g, ' ');
          document.execCommand('insertText', false, text);
        }}
        dangerouslySetInnerHTML={{ __html: html }}
      />
      <button type="button" className="cs-line__bullet-toggle" tabIndex={-1} aria-label={line.bullet ? 'Remove bullet' : 'Make bullet'} title={line.bullet ? 'Remove bullet' : 'Make bullet'} onMouseDown={(e) => e.preventDefault()} onClick={onToggleBullet}>
        •
      </button>
    </div>
  );
}

function caretOffset(el: HTMLElement, range: Range): number {
  const pre = range.cloneRange();
  pre.selectNodeContents(el);
  pre.setEnd(range.startContainer, range.startOffset);
  return pre.toString().length;
}

function isOnFirstLine(el: HTMLElement): boolean {
  const selection = window.getSelection();
  if (!selection || selection.rangeCount === 0) return true;
  const rect = selection.getRangeAt(0).getBoundingClientRect();
  const box = el.getBoundingClientRect();
  return rect.height === 0 || rect.top - box.top < rect.height * 1.2;
}

function isOnLastLine(el: HTMLElement): boolean {
  const selection = window.getSelection();
  if (!selection || selection.rangeCount === 0) return true;
  const rect = selection.getRangeAt(0).getBoundingClientRect();
  const box = el.getBoundingClientRect();
  return rect.height === 0 || box.bottom - rect.bottom < rect.height * 1.2;
}
