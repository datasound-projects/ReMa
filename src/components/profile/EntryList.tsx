import { useState, type ReactNode } from 'react';

import { move, removeAt, replaceAt } from '../../lib/profileMerge';
import { ChevronDownIcon, ChevronUpIcon, PlusIcon, TrashIcon } from '../icons';
import { IconButton } from '../ui/IconButton';

interface EntryListProps<T> {
  items: T[];
  onChange: (items: T[]) => void;
  /** One-line summary of a collapsed entry. */
  summary: (item: T) => ReactNode;
  /** The form of an expanded entry. */
  editor: (item: T, update: (item: T) => void) => ReactNode;
  create: () => T;
  addLabel: string;
  empty: string;
}

/** Ordered entries (experience, education) with progressive disclosure. */
export function EntryList<T>({ items, onChange, summary, editor, create, addLabel, empty }: EntryListProps<T>) {
  const [openIndex, setOpenIndex] = useState<number | null>(null);

  return (
    <div className="entries">
      {items.length === 0 && <p className="entries__empty">{empty}</p>}
      {items.map((item, index) => {
        const open = openIndex === index;
        return (
          <div key={index} className={open ? 'entry entry--open' : 'entry'}>
            <div className="entry__row">
              <button
                type="button"
                className="entry__summary"
                aria-expanded={open}
                onClick={() => setOpenIndex(open ? null : index)}
              >
                {summary(item)}
              </button>
              <IconButton
                label="Move up"
                className="icon-button--small"
                disabled={index === 0}
                onClick={() => {
                  onChange(move(items, index, -1));
                  setOpenIndex(null);
                }}
              >
                <ChevronUpIcon />
              </IconButton>
              <IconButton
                label="Move down"
                className="icon-button--small"
                disabled={index === items.length - 1}
                onClick={() => {
                  onChange(move(items, index, 1));
                  setOpenIndex(null);
                }}
              >
                <ChevronDownIcon />
              </IconButton>
              <IconButton
                label="Remove"
                className="icon-button--small"
                onClick={() => {
                  onChange(removeAt(items, index));
                  setOpenIndex(null);
                }}
              >
                <TrashIcon />
              </IconButton>
            </div>
            {open && <div className="entry__form">{editor(item, (next) => onChange(replaceAt(items, index, next)))}</div>}
          </div>
        );
      })}
      <button
        type="button"
        className="button button--ghost entries__add"
        onClick={() => {
          onChange([...items, create()]);
          setOpenIndex(items.length);
        }}
      >
        <PlusIcon className="button__icon" />
        {addLabel}
      </button>
    </div>
  );
}

/** Skills as removable chips; Enter or comma adds. */
export function ChipInput({
  values,
  onChange,
  placeholder,
  label,
}: {
  values: string[];
  onChange: (values: string[]) => void;
  placeholder: string;
  label: string;
}) {
  const [text, setText] = useState('');
  const add = (raw: string) => {
    const parts = raw
      .split(',')
      .map((s) => s.trim())
      .filter((s) => s && !values.some((v) => v.toLowerCase() === s.toLowerCase()));
    if (parts.length) onChange([...values, ...parts]);
    setText('');
  };
  return (
    <div className="chips">
      {values.map((value, index) => (
        <span key={value} className="chip">
          {value}
          <button
            type="button"
            className="chip__remove"
            aria-label={`Remove ${value}`}
            onClick={() => onChange(removeAt(values, index))}
          >
            ×
          </button>
        </span>
      ))}
      <input
        className="chips__input"
        aria-label={label}
        placeholder={values.length === 0 ? placeholder : 'Add…'}
        value={text}
        onChange={(e) => {
          if (e.target.value.includes(',')) add(e.target.value);
          else setText(e.target.value);
        }}
        onKeyDown={(e) => {
          if (e.key === 'Enter') {
            e.preventDefault();
            add(text);
          } else if (e.key === 'Backspace' && !text && values.length) {
            onChange(values.slice(0, -1));
          }
        }}
        onBlur={() => text.trim() && add(text)}
      />
    </div>
  );
}
