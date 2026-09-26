import { useState } from 'react';

import { ENTRY_FIELDS, HAS_TEXT, SECTION_KINDS, moveEntry, newEntry, type EntryFields } from '../../lib/portfolio/content';
import { period } from '../../lib/portfolio/layout';
import type { PortfolioEntry, PortfolioSection } from '../../services/portfolioService';
import { ChevronDownIcon, ChevronRightIcon, ChevronUpIcon, EyeIcon, EyeOffIcon, PlusIcon, TrashIcon } from '../icons';
import { ChipInput } from '../profile/EntryList';
import { IconButton } from '../ui/IconButton';

interface SectionEditorProps {
  section: PortfolioSection;
  first: boolean;
  last: boolean;
  onChange: (section: PortfolioSection) => void;
  onMove: (delta: -1 | 1) => void;
  onRemove: () => void;
}

const kindLabel = (s: PortfolioSection) => SECTION_KINDS.find((k) => k.kind === s.kind)?.label ?? 'Section';

/** One section of a CV: its title, visibility, order, text and entries. */
export function SectionEditor({ section, first, last, onChange, onMove, onRemove }: SectionEditorProps) {
  const [open, setOpen] = useState(true);
  const [confirming, setConfirming] = useState(false);
  const fields = ENTRY_FIELDS[section.kind];
  const set = (patch: Partial<PortfolioSection>) => onChange({ ...section, ...patch });
  const setEntry = (id: string, entry: PortfolioEntry) =>
    set({ entries: section.entries.map((e) => (e.id === id ? entry : e)) });

  return (
    <section
      className={section.visible ? 'studio-section' : 'studio-section studio-section--hidden'}
      aria-label={`${section.title} section`}
    >
      <div className="studio-section__head">
        <IconButton
          label={open ? 'Collapse section' : 'Expand section'}
          className="icon-button--small"
          aria-expanded={open}
          onClick={() => setOpen(!open)}
        >
          {open ? <ChevronDownIcon /> : <ChevronRightIcon />}
        </IconButton>
        <input
          className="input input--small studio-section__title"
          aria-label="Section title"
          maxLength={200}
          value={section.title}
          onChange={(e) => set({ title: e.target.value })}
        />
        <span className="studio-section__kind">{kindLabel(section)}</span>
        <IconButton
          label={section.visible ? 'Hide section' : 'Show section'}
          className="icon-button--small"
          aria-pressed={!section.visible}
          onClick={() => set({ visible: !section.visible })}
        >
          {section.visible ? <EyeIcon /> : <EyeOffIcon />}
        </IconButton>
        <IconButton label="Move section up" className="icon-button--small" disabled={first} onClick={() => onMove(-1)}>
          <ChevronUpIcon />
        </IconButton>
        <IconButton label="Move section down" className="icon-button--small" disabled={last} onClick={() => onMove(1)}>
          <ChevronDownIcon />
        </IconButton>
        <IconButton label="Remove section" className="icon-button--small" onClick={() => setConfirming(true)}>
          <TrashIcon />
        </IconButton>
      </div>
      {confirming && (
        <div className="doc-card__confirm studio-section__confirm" role="group" aria-label="Confirm removal">
          <span>Remove “{section.title}” and its content?</span>
          <button type="button" className="button button--danger button--small" onClick={onRemove}>
            Remove
          </button>
          <button type="button" className="button button--ghost button--small" onClick={() => setConfirming(false)}>
            Cancel
          </button>
        </div>
      )}
      {!section.visible && <p className="studio-section__note">Hidden: kept here, left out of the PDF.</p>}
      {open && (
        <div className="studio-section__body">
          {HAS_TEXT[section.kind] && (
            <label className="field">
              <span className="field__label">{section.kind === 'summary' ? 'Summary' : 'Text'}</span>
              <textarea
                className="input input--textarea"
                rows={section.kind === 'summary' ? 4 : 3}
                maxLength={8000}
                value={section.text}
                placeholder={section.kind === 'summary' ? 'Two or three sentences about you and what you do best.' : ''}
                onChange={(e) => set({ text: e.target.value })}
              />
            </label>
          )}
          {fields && (
            <Entries
              section={section}
              fields={fields}
              onEntry={setEntry}
              onEntries={(entries) => set({ entries })}
              onMove={(id, delta) => onChange(moveEntry(section, id, delta))}
            />
          )}
        </div>
      )}
    </section>
  );
}

function Entries({
  section,
  fields,
  onEntry,
  onEntries,
  onMove,
}: {
  section: PortfolioSection;
  fields: EntryFields;
  onEntry: (id: string, entry: PortfolioEntry) => void;
  onEntries: (entries: PortfolioEntry[]) => void;
  onMove: (id: string, delta: -1 | 1) => void;
}) {
  const simple = section.kind === 'languages' || section.kind === 'links' || section.kind === 'skills';
  const [openId, setOpenId] = useState<string | null>(null);
  const entries = section.entries;
  const add = () => {
    const entry = newEntry();
    onEntries([...entries, entry]);
    setOpenId(entry.id);
  };
  const remove = (id: string) => onEntries(entries.filter((e) => e.id !== id));
  const addLabel =
    section.kind === 'skills' ? 'Add skill group' : section.kind === 'languages' ? 'Add language' : section.kind === 'links' ? 'Add link' : 'Add entry';

  return (
    <div className="entries">
      {entries.length === 0 && <p className="entries__empty">Nothing here yet.</p>}
      {entries.map((entry, index) => {
        const controls = (
          <>
            <IconButton label="Move up" className="icon-button--small" disabled={index === 0} onClick={() => onMove(entry.id, -1)}>
              <ChevronUpIcon />
            </IconButton>
            <IconButton
              label="Move down"
              className="icon-button--small"
              disabled={index === entries.length - 1}
              onClick={() => onMove(entry.id, 1)}
            >
              <ChevronDownIcon />
            </IconButton>
            <IconButton label="Remove" className="icon-button--small" onClick={() => remove(entry.id)}>
              <TrashIcon />
            </IconButton>
          </>
        );
        if (simple) {
          return (
            <div key={entry.id} className="studio-row">
              <SimpleEntry kind={section.kind} fields={fields} entry={entry} onChange={(e) => onEntry(entry.id, e)} />
              <div className="studio-row__controls">{controls}</div>
            </div>
          );
        }
        const open = openId === entry.id;
        const summary = [entry.title, entry.subtitle].map((s) => s.trim()).filter(Boolean).join(' · ');
        return (
          <div key={entry.id} className={open ? 'entry entry--open' : 'entry'}>
            <div className="entry__row">
              <button type="button" className="entry__summary" aria-expanded={open} onClick={() => setOpenId(open ? null : entry.id)}>
                <span className="entry__title">{summary || `New ${fields.title.toLowerCase()}`}</span>
                <span className="entry__meta">{period(entry)}</span>
              </button>
              {controls}
            </div>
            {open && (
              <div className="entry__form">
                <RichEntry fields={fields} entry={entry} onChange={(e) => onEntry(entry.id, e)} />
              </div>
            )}
          </div>
        );
      })}
      <button type="button" className="button button--ghost entries__add" onClick={add}>
        <PlusIcon className="button__icon" />
        {addLabel}
      </button>
    </div>
  );
}

function TextField({
  label,
  value,
  onChange,
  placeholder,
  type = 'text',
  grow,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  type?: 'text' | 'url';
  grow?: boolean;
}) {
  return (
    <label className={grow ? 'field field--grow' : 'field'}>
      <span className="field__label">{label}</span>
      <input
        className="input"
        type={type}
        value={value}
        placeholder={placeholder}
        maxLength={type === 'url' ? 2000 : 200}
        onChange={(e) => onChange(e.target.value)}
      />
    </label>
  );
}

function SimpleEntry({
  kind,
  fields,
  entry,
  onChange,
}: {
  kind: PortfolioSection['kind'];
  fields: EntryFields;
  entry: PortfolioEntry;
  onChange: (entry: PortfolioEntry) => void;
}) {
  if (kind === 'skills') {
    return (
      <div className="studio-row__fields studio-row__fields--stack">
        <TextField label={fields.title} value={entry.title} placeholder="e.g. Data" onChange={(title) => onChange({ ...entry, title })} />
        <div className="field">
          <span className="field__label">{fields.tags ?? 'Skills'}</span>
          <ChipInput
            label="Add a skill"
            placeholder="Type a skill and press Enter"
            values={entry.tags}
            onChange={(tags) => onChange({ ...entry, tags })}
          />
        </div>
      </div>
    );
  }
  return (
    <div className="studio-row__fields">
      <TextField
        label={fields.title}
        value={entry.title}
        placeholder={kind === 'languages' ? 'English' : 'Portfolio'}
        onChange={(title) => onChange({ ...entry, title })}
      />
      {fields.subtitle && (
        <TextField
          label={fields.subtitle}
          value={entry.subtitle}
          placeholder="C1, native…"
          onChange={(subtitle) => onChange({ ...entry, subtitle })}
          grow
        />
      )}
      {fields.url && (
        <TextField label="URL" type="url" value={entry.url} placeholder="https://" onChange={(url) => onChange({ ...entry, url })} grow />
      )}
    </div>
  );
}

function RichEntry({
  fields,
  entry,
  onChange,
}: {
  fields: EntryFields;
  entry: PortfolioEntry;
  onChange: (entry: PortfolioEntry) => void;
}) {
  const set = (patch: Partial<PortfolioEntry>) => onChange({ ...entry, ...patch });
  return (
    <>
      <div className="profile-grid">
        <TextField label={fields.title} value={entry.title} onChange={(title) => set({ title })} />
        {fields.subtitle && <TextField label={fields.subtitle} value={entry.subtitle} onChange={(subtitle) => set({ subtitle })} />}
        {fields.location && <TextField label="Location" value={entry.location} onChange={(location) => set({ location })} />}
        {fields.dates === 'range' && (
          <div className="form__row">
            <TextField label="Start" placeholder="2021-03" value={entry.start} onChange={(start) => set({ start })} />
            <TextField label="End" placeholder="Present" value={entry.end} onChange={(end) => set({ end })} />
          </div>
        )}
        {fields.dates === 'single' && (
          <TextField label="Date" placeholder="2024-05" value={entry.end} onChange={(end) => set({ end, start: '' })} />
        )}
        {fields.url && (
          <TextField label="Link" type="url" placeholder="https://" value={entry.url} onChange={(url) => set({ url })} />
        )}
      </div>
      {fields.description && (
        <label className="field">
          <span className="field__label">Description</span>
          <textarea
            className="input input--textarea"
            rows={4}
            maxLength={5000}
            value={entry.description}
            placeholder={'Start a line with “- ” for a bullet point.'}
            onChange={(e) => set({ description: e.target.value })}
          />
        </label>
      )}
      {fields.tags && (
        <div className="field">
          <span className="field__label">{fields.tags}</span>
          <ChipInput
            label={`Add to ${fields.tags.toLowerCase()}`}
            placeholder="Type and press Enter"
            values={entry.tags}
            onChange={(tags) => set({ tags })}
          />
        </div>
      )}
    </>
  );
}
