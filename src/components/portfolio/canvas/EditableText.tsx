import { useEffect, useRef, type CSSProperties, type KeyboardEvent } from 'react';

import type { FieldTarget } from '../../../lib/portfolio/fields';

interface EditableTextProps {
  value: string;
  onChange: (value: string) => void;
  target: FieldTarget;
  className?: string;
  style?: CSSProperties;
  placeholder?: string;
  /** Enter inserts a line break instead of finishing the edit. */
  multiline?: boolean;
  /** An element name, e.g. "h1". */
  as?: 'span' | 'div' | 'h1' | 'h2' | 'h3' | 'p';
  /** Accessible name of the field. */
  label: string;
  onFocus?: () => void;
}

/** Click-to-edit plain text on the canvas (single field, no formatting). */
export function EditableText({ value, onChange, target, className, style, placeholder, multiline, as = 'span', label, onFocus }: EditableTextProps) {
  const ref = useRef<HTMLElement>(null);
  const Tag = as as 'span';

  // Keep the DOM in step with the value unless the field is being edited.
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    if (document.activeElement === el) return;
    if (el.textContent !== value) el.textContent = value;
  }, [value]);

  const commit = () => {
    const el = ref.current;
    if (!el) return;
    const text = (el.textContent ?? '').replace(/ /g, ' ');
    const next = multiline ? text.replace(/\r/g, '') : text.replace(/[\r\n]+/g, ' ');
    if (next !== value) onChange(next);
  };

  const onKeyDown = (event: KeyboardEvent<HTMLElement>) => {
    if (event.key === 'Enter' && !multiline) {
      event.preventDefault();
      ref.current?.blur();
    }
    if (event.key === 'Escape') ref.current?.blur();
  };

  return (
    <Tag
      ref={ref as never}
      className={`cs-text${className ? ` ${className}` : ''}${value ? '' : ' cs-text--empty'}`}
      style={style}
      contentEditable
      suppressContentEditableWarning
      role="textbox"
      aria-label={label}
      aria-multiline={multiline || undefined}
      data-placeholder={placeholder}
      data-target={JSON.stringify(target)}
      spellCheck
      onInput={commit}
      onBlur={commit}
      onFocus={onFocus}
      onKeyDown={onKeyDown}
      onPaste={(event) => {
        event.preventDefault();
        const text = event.clipboardData.getData('text/plain');
        document.execCommand('insertText', false, multiline ? text : text.replace(/[\r\n]+/g, ' '));
      }}
    >
      {value}
    </Tag>
  );
}
