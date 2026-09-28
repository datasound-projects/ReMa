import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type CSSProperties, type ReactNode } from 'react';

import { CANVAS_FAMILY, CANVAS_WEIGHT, loadCanvasFonts } from '../../../lib/portfolio/canvasFonts';
import { ENTRY_FIELDS, SECTION_KINDS, addSection, moveEntry, moveSection, newEntry, newId, removeSection, updateSection } from '../../../lib/portfolio/content';
import { isRichTarget, type FieldTarget } from '../../../lib/portfolio/fields';
import { PAGE_SIZES, displayUrl, formatDate, tint, type Theme } from '../../../lib/portfolio/layout';
import type { CoverLetter, PortfolioEntry, PortfolioInput, PortfolioSection, SectionKind } from '../../../services/portfolioService';
import { ChevronDownIcon, ChevronUpIcon, CopyIcon, EyeIcon, EyeOffIcon, LinkIcon, PlusIcon, SparkleIcon, TrashIcon } from '../../icons';
import { Menu } from '../../ui/Menu';
import { EditableText } from './EditableText';
import { LineEditor } from './LineEditor';

export type CanvasSelection = { kind: 'header' } | { kind: 'section'; id: string } | { kind: 'entry'; sectionId: string; entryId: string } | null;

export interface TextSelection {
  text: string;
  target: FieldTarget;
}

interface DocumentCanvasProps {
  draft: PortfolioInput;
  theme: Theme;
  edit: (update: (d: PortfolioInput) => PortfolioInput, group?: string) => void;
  selection: CanvasSelection;
  onSelect: (selection: CanvasSelection) => void;
  onTextSelection: (selection: TextSelection | null) => void;
  onAskAi: (selection: TextSelection) => void;
  /** Sheet width as a fraction of the available width (1 = fit). */
  zoom: number;
  photoCircle?: string;
}

const ptToPx = (pt: number, scale: number) => `${(pt * scale).toFixed(2)}px`;

/**
 * The document as an editable page: click any text to edit it, select
 * text to format it or send it to the AI assistant, select a section or
 * entry to move, hide, duplicate or delete it. The same document model
 * drives the forms and the exported PDF.
 */
export function DocumentCanvas({ draft, theme, edit, selection, onSelect, onTextSelection, onAskAi, zoom, photoCircle }: DocumentCanvasProps) {
  const root = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(0);
  const [toolbar, setToolbar] = useState<{ x: number; y: number; selection: TextSelection } | null>(null);
  const [linking, setLinking] = useState<string | null>(null);
  const [fontsReady, setFontsReady] = useState(false);
  const { t } = theme;
  const page = PAGE_SIZES[draft.pageSize];
  const isLetter = (draft.kind ?? 'cv') === 'cover_letter';

  useEffect(() => {
    void loadCanvasFonts().then(() => setFontsReady(true));
  }, []);

  useLayoutEffect(() => {
    const el = root.current;
    if (!el) return;
    const observer = new ResizeObserver((entries) => setWidth(Math.floor(entries[0]?.contentRect.width ?? 0)));
    observer.observe(el);
    setWidth(Math.floor(el.clientWidth));
    return () => observer.disconnect();
  }, []);

  const sheetWidth = Math.max(320, Math.floor(width * zoom));
  const scale = sheetWidth / page.width;
  const pageHeight = page.height * scale;

  const vars = useMemo(() => {
    const s = scale;
    const style: Record<string, string> = {
      '--cs-scale': String(s),
      '--cs-font-body': CANVAS_FAMILY[t.fonts.body],
      '--cs-font-heading': CANVAS_FAMILY[t.fonts.heading],
      '--cs-font-name': CANVAS_FAMILY[t.fonts.name],
      '--cs-font-label': CANVAS_FAMILY[t.fonts.label ?? t.fonts.heading],
      '--cs-weight-body': String(CANVAS_WEIGHT[t.fonts.body]),
      '--cs-weight-heading': String(Math.max(600, CANVAS_WEIGHT[t.fonts.heading])),
      '--cs-weight-name': String(Math.max(600, CANVAS_WEIGHT[t.fonts.name])),
      '--cs-body': ptToPx(t.size.body, s),
      '--cs-small': ptToPx(t.size.small, s),
      '--cs-name': ptToPx(t.size.name, s),
      '--cs-headline': ptToPx(t.size.headline, s),
      '--cs-section': ptToPx(t.size.section, s),
      '--cs-title': ptToPx(t.size.title, s),
      '--cs-line': String(t.lineHeight),
      '--cs-text': t.colors.text,
      '--cs-muted': t.colors.muted,
      '--cs-accent': theme.accent,
      '--cs-heading': theme.heading,
      '--cs-rule': t.colors.rule,
      '--cs-tag': theme.tag,
      '--cs-panel': theme.panel,
      '--cs-panel-text': theme.panelText,
      '--cs-panel-muted': theme.panelMuted,
      '--cs-bg': t.colors.background ?? '#ffffff',
      '--cs-band-soft': tint(theme.accent, 0.78),
      '--cs-margin-l': ptToPx(t.margins[0], s),
      '--cs-margin-t': ptToPx(t.margins[1], s),
      '--cs-margin-r': ptToPx(t.margins[2], s),
      '--cs-margin-b': ptToPx(t.margins[3], s),
      '--cs-gap-section': ptToPx(t.gap.section, s),
      '--cs-gap-entry': ptToPx(t.gap.entry, s),
      '--cs-side-width': ptToPx(t.sidebar?.width ?? 0, s),
      '--cs-gutter': ptToPx(t.layout === 'sidebar' ? 30 : 26, s),
      '--cs-page-height': `${pageHeight.toFixed(1)}px`,
      '--cs-text-align': t.justify ? 'justify' : 'left',
    };
    return style as CSSProperties;
  }, [scale, t, theme, pageHeight]);

  // Text selection → floating toolbar.
  useEffect(() => {
    const onChange = () => {
      const sel = window.getSelection();
      const el = root.current;
      if (!sel || !el || sel.rangeCount === 0 || sel.isCollapsed) {
        setToolbar(null);
        onTextSelection(null);
        return;
      }
      const range = sel.getRangeAt(0);
      const node = range.commonAncestorContainer;
      const element = node.nodeType === Node.ELEMENT_NODE ? (node as HTMLElement) : node.parentElement;
      const field = element?.closest<HTMLElement>('[data-target]');
      if (!field || !el.contains(field)) {
        setToolbar(null);
        onTextSelection(null);
        return;
      }
      const text = sel.toString();
      if (!text.trim()) {
        setToolbar(null);
        onTextSelection(null);
        return;
      }
      const target = JSON.parse(field.dataset.target ?? '{}') as FieldTarget;
      const rect = range.getBoundingClientRect();
      const box = el.getBoundingClientRect();
      const selection = { text, target };
      setToolbar({ x: rect.left - box.left + rect.width / 2, y: rect.top - box.top + el.scrollTop, selection });
      onTextSelection(selection);
    };
    document.addEventListener('selectionchange', onChange);
    return () => document.removeEventListener('selectionchange', onChange);
  }, [onTextSelection]);

  const format = (command: 'bold' | 'italic' | 'createLink', value?: string) => {
    const sel = window.getSelection();
    if (!sel || sel.rangeCount === 0) return;
    const node = sel.getRangeAt(0).commonAncestorContainer;
    const element = node.nodeType === Node.ELEMENT_NODE ? (node as HTMLElement) : node.parentElement;
    const line = element?.closest<HTMLElement>('[data-line]');
    if (!line) return;
    document.execCommand(command, false, value);
    line.dispatchEvent(new Event('input', { bubbles: true }));
  };

  const setSection = useCallback(
    (id: string, update: (s: PortfolioSection) => PortfolioSection, group?: string) =>
      edit((d) => ({ ...d, content: updateSection(d.content, id, update) }), group),
    [edit],
  );

  const sectionActions = (section: PortfolioSection, index: number, count: number) => ({
    moveUp: () => edit((d) => ({ ...d, content: moveSection(d.content, section.id, -1) })),
    moveDown: () => edit((d) => ({ ...d, content: moveSection(d.content, section.id, 1) })),
    canUp: index > 0,
    canDown: index < count - 1,
    toggle: () => setSection(section.id, (s) => ({ ...s, visible: !s.visible })),
    duplicate: () =>
      edit((d) => {
        const i = d.content.sections.findIndex((s) => s.id === section.id);
        const copy: PortfolioSection = { ...section, id: newId(), title: `${section.title} (copy)`, entries: section.entries.map((e) => ({ ...e, id: newId() })) };
        const sections = [...d.content.sections];
        sections.splice(i + 1, 0, copy);
        return { ...d, content: { ...d.content, sections } };
      }),
    remove: () => {
      edit((d) => ({ ...d, content: removeSection(d.content, section.id) }));
      onSelect(null);
    },
    addEntry: () => {
      const entry = newEntry();
      setSection(section.id, (s) => ({ ...s, entries: [...s.entries, entry] }));
      onSelect({ kind: 'entry', sectionId: section.id, entryId: entry.id });
    },
  });

  const entryActions = (section: PortfolioSection, entry: PortfolioEntry, index: number) => ({
    moveUp: () => setSection(section.id, (s) => moveEntry(s, entry.id, -1)),
    moveDown: () => setSection(section.id, (s) => moveEntry(s, entry.id, 1)),
    canUp: index > 0,
    canDown: index < section.entries.length - 1,
    duplicate: () =>
      setSection(section.id, (s) => {
        const i = s.entries.findIndex((e) => e.id === entry.id);
        const entries = [...s.entries];
        entries.splice(i + 1, 0, { ...entry, id: newId() });
        return { ...s, entries };
      }),
    remove: () => {
      setSection(section.id, (s) => ({ ...s, entries: s.entries.filter((e) => e.id !== entry.id) }));
      onSelect({ kind: 'section', id: section.id });
    },
  });

  const sideKinds = t.sidebar && !t.plain ? t.sidebar.kinds : [];
  const sections = draft.content.sections;
  const mainSections = sections.filter((s) => !sideKinds.includes(s.kind));
  const sideSections = sections.filter((s) => sideKinds.includes(s.kind));
  const layout = t.plain ? 'single' : t.layout;
  const panel = layout === 'sidebar';
  const sideStyle = t.sidebar?.style ?? (panel ? 'tinted' : 'plain');
  const tinted = panel && sideStyle === 'tinted';
  const h = draft.content.header;
  const showPhoto = !!draft.style?.showPhoto && !!h.photo && !t.plain;
  const photoSrc = t.photo === 'circle' && photoCircle ? photoCircle : h.photo;

  const renderSection = (section: PortfolioSection, index: number, list: PortfolioSection[], side: boolean) => {
    const actions = sectionActions(section, sections.indexOf(section), sections.length);
    const selected = selection?.kind === 'section' && selection.id === section.id;
    const fields = ENTRY_FIELDS[section.kind];
    const first = index === 0;
    return (
      <section
        key={section.id}
        className={[
          'cs-section',
          `cs-section--${section.kind}`,
          `cs-section--style-${side && !['caps', 'rule', 'plain', 'thin-caps', 'mono', 'serif'].includes(t.section) ? 'caps' : t.section}`,
          side ? 'cs-section--side' : '',
          selected ? 'cs-section--selected' : '',
          section.visible ? '' : 'cs-section--hidden',
          first ? 'cs-section--first' : '',
          index === list.length - 1 ? 'cs-section--last' : '',
          t.dividers && t.dividers !== 'none' && !t.plain ? `cs-section--divider-${t.dividers}` : '',
        ]
          .filter(Boolean)
          .join(' ')}
        data-section={section.id}
        aria-label={`${section.title} section`}
        onMouseDown={(e) => {
          if ((e.target as HTMLElement).closest('.cs-actions, [contenteditable], button')) return;
          onSelect({ kind: 'section', id: section.id });
        }}
      >
        <div className="cs-section__title-row">
          {t.section === 'numbered' && !side && <span className="cs-section__number">{String(index + 1).padStart(2, '0')}</span>}
          {t.section === 'mono' && <span className="cs-section__prefix">// </span>}
          {t.section === 'bar' && !side && <span className="cs-section__bar" aria-hidden="true" />}
          {t.section === 'side-rule' && !side && <span className="cs-section__bar cs-section__bar--tall" aria-hidden="true" />}
          <EditableText
            as="h2"
            className="cs-section__title"
            label={`${section.title} title`}
            value={section.title}
            placeholder="Section title"
            target={{ scope: 'section', sectionId: section.id, field: 'title' }}
            onChange={(title) => setSection(section.id, (s) => ({ ...s, title }), `title-${section.id}`)}
            onFocus={() => onSelect({ kind: 'section', id: section.id })}
          />
          {!section.visible && <span className="cs-section__hidden-note">Hidden · not exported</span>}
          <div className="cs-actions cs-actions--section" role="toolbar" aria-label={`${section.title} actions`}>
            <button type="button" className="cs-actions__button" title="Move up" aria-label="Move section up" disabled={!actions.canUp} onClick={actions.moveUp}>
              <ChevronUpIcon />
            </button>
            <button type="button" className="cs-actions__button" title="Move down" aria-label="Move section down" disabled={!actions.canDown} onClick={actions.moveDown}>
              <ChevronDownIcon />
            </button>
            <button type="button" className="cs-actions__button" title={section.visible ? 'Hide section' : 'Show section'} aria-label={section.visible ? 'Hide section' : 'Show section'} onClick={actions.toggle}>
              {section.visible ? <EyeOffIcon /> : <EyeIcon />}
            </button>
            <button type="button" className="cs-actions__button" title="Duplicate section" aria-label="Duplicate section" onClick={actions.duplicate}>
              <CopyIcon />
            </button>
            {fields && (
              <button type="button" className="cs-actions__button" title="Add entry" aria-label="Add entry" onClick={actions.addEntry}>
                <PlusIcon />
              </button>
            )}
            <button type="button" className="cs-actions__button cs-actions__button--danger" title="Delete section" aria-label="Delete section" onClick={actions.remove}>
              <TrashIcon />
            </button>
          </div>
        </div>
        {(section.kind === 'summary' || section.kind === 'custom' || section.text) && (
          <LineEditor
            className="cs-section__text"
            label={`${section.title} text`}
            value={section.text}
            placeholder={section.kind === 'summary' ? 'Two or three sentences about you. Start a line with “- ” for a bullet.' : 'Text…'}
            target={{ scope: 'section', sectionId: section.id, field: 'text' }}
            onChange={(text) => setSection(section.id, (s) => ({ ...s, text }), `text-${section.id}`)}
            onFocus={() => onSelect({ kind: 'section', id: section.id })}
          />
        )}
        {fields && renderEntries(section, side)}
        {fields && (
          <button type="button" className="cs-add-entry" onClick={actions.addEntry}>
            <PlusIcon /> Add {fields.title.toLowerCase().replace(' (optional)', '')}
          </button>
        )}
      </section>
    );
  };

  const renderEntries = (section: PortfolioSection, side: boolean) => {
    const kind = section.kind;
    const entryStyle = side || t.plain ? (t.plain ? 'stacked' : 'stacked') : t.entry;
    return (
      <div className={`cs-entries cs-entries--${kind} cs-entries--${entryStyle}`}>
        {section.entries.map((entry, index) => {
          const actions = entryActions(section, entry, index);
          const selected = selection?.kind === 'entry' && selection.entryId === entry.id;
          const target = (field: Exclude<keyof PortfolioEntry, 'id' | 'tags'>): FieldTarget => ({ scope: 'entry', sectionId: section.id, entryId: entry.id, field });
          const set = (field: Exclude<keyof PortfolioEntry, 'id' | 'tags'>) => (value: string) =>
            setSection(section.id, (s) => ({ ...s, entries: s.entries.map((e) => (e.id === entry.id ? { ...e, [field]: value } : e)) }), `${entry.id}-${field}`);
          const setTags = (tags: string[]) => setSection(section.id, (s) => ({ ...s, entries: s.entries.map((e) => (e.id === entry.id ? { ...e, tags } : e)) }));
          const focus = () => onSelect({ kind: 'entry', sectionId: section.id, entryId: entry.id });
          const controls = (
            <div className="cs-actions cs-actions--entry" role="toolbar" aria-label="Entry actions">
              <button type="button" className="cs-actions__button" title="Move up" aria-label="Move entry up" disabled={!actions.canUp} onClick={actions.moveUp}>
                <ChevronUpIcon />
              </button>
              <button type="button" className="cs-actions__button" title="Move down" aria-label="Move entry down" disabled={!actions.canDown} onClick={actions.moveDown}>
                <ChevronDownIcon />
              </button>
              <button type="button" className="cs-actions__button" title="Duplicate entry" aria-label="Duplicate entry" onClick={actions.duplicate}>
                <CopyIcon />
              </button>
              <button type="button" className="cs-actions__button cs-actions__button--danger" title="Delete entry" aria-label="Delete entry" onClick={actions.remove}>
                <TrashIcon />
              </button>
            </div>
          );
          const wrap = (children: ReactNode, className = '') => (
            <div
              key={entry.id}
              className={`cs-entry ${className}${selected ? ' cs-entry--selected' : ''}`}
              data-entry={entry.id}
              onMouseDown={(e) => {
                if ((e.target as HTMLElement).closest('.cs-actions, button')) return;
                onSelect({ kind: 'entry', sectionId: section.id, entryId: entry.id });
              }}
            >
              {children}
              {controls}
            </div>
          );
          const dates = (
            <span className="cs-entry__dates">
              <EditableText label="Start" value={entry.start} placeholder="2021-03" target={target('start')} onChange={set('start')} onFocus={focus} />
              <span className="cs-entry__dash">{entry.start || entry.end ? ' – ' : ''}</span>
              <EditableText label="End" value={entry.end} placeholder="Present" target={target('end')} onChange={set('end')} onFocus={focus} />
            </span>
          );
          const singleDate = <EditableText className="cs-entry__dates" label="Date" value={entry.end} placeholder="2024" target={target('end')} onChange={set('end')} onFocus={focus} />;

          if (kind === 'skills') {
            return wrap(
              <div className={`cs-skills cs-skills--${side && t.skills === 'list' ? 'list' : t.plain ? 'inline' : t.skills}`}>
                <EditableText className="cs-skills__label" label="Skill group" value={entry.title} placeholder="Group (optional)" target={target('title')} onChange={set('title')} onFocus={focus} />
                <TagEditor tags={entry.tags} onChange={setTags} placeholder="Add a skill" chips={t.skills === 'tags' && !t.plain && !side} list={side && t.skills === 'list'} />
              </div>,
              'cs-entry--skills',
            );
          }
          if (kind === 'languages') {
            return wrap(
              <div className="cs-language">
                <EditableText className="cs-language__name" label="Language" value={entry.title} placeholder="Language" target={target('title')} onChange={set('title')} onFocus={focus} />
                <EditableText className="cs-language__level" label="Level" value={entry.subtitle} placeholder="Level" target={target('subtitle')} onChange={set('subtitle')} onFocus={focus} />
              </div>,
              'cs-entry--language',
            );
          }
          if (kind === 'links') {
            return wrap(
              <div className="cs-link">
                <EditableText className="cs-link__label" label="Link label" value={entry.title} placeholder="Label" target={target('title')} onChange={set('title')} onFocus={focus} />
                <EditableText className="cs-link__url" label="URL" value={entry.url} placeholder="https://" target={target('url')} onChange={set('url')} onFocus={focus} />
              </div>,
              'cs-entry--link',
            );
          }
          if (kind === 'publications') {
            return wrap(
              <div className="cs-publication">
                <EditableText className="cs-publication__title" label="Title" value={entry.title} placeholder="Title" target={target('title')} onChange={set('title')} onFocus={focus} />
                <span className="cs-publication__meta">
                  <EditableText className="cs-publication__venue" label="Journal, conference or publisher" value={entry.subtitle} placeholder="Journal or venue" target={target('subtitle')} onChange={set('subtitle')} onFocus={focus} />
                  {singleDate}
                  <EditableText className="cs-publication__where" label="Co-authors or place" value={entry.location} placeholder="" target={target('location')} onChange={set('location')} onFocus={focus} />
                </span>
                <LineEditor className="cs-entry__description" label="Description" value={entry.description} placeholder="" target={target('description')} onChange={set('description')} onFocus={focus} />
                <EditableText className="cs-entry__url" label="Link" value={entry.url} placeholder="https://" target={target('url')} onChange={set('url')} onFocus={focus} />
              </div>,
              'cs-entry--publication',
            );
          }
          const certification = kind === 'certifications';
          const title = <EditableText as="h3" className="cs-entry__title" label={ENTRY_FIELDS[kind]?.title ?? 'Title'} value={entry.title} placeholder={ENTRY_FIELDS[kind]?.title ?? 'Title'} target={target('title')} onChange={set('title')} onFocus={focus} />;
          const subtitle = (
            <span className="cs-entry__subtitle">
              {ENTRY_FIELDS[kind]?.subtitle && <EditableText label={ENTRY_FIELDS[kind]?.subtitle ?? 'Subtitle'} value={entry.subtitle} placeholder={ENTRY_FIELDS[kind]?.subtitle ?? ''} target={target('subtitle')} onChange={set('subtitle')} onFocus={focus} />}
              {ENTRY_FIELDS[kind]?.location && (
                <>
                  <span className="cs-sep">{entry.location || selected ? ' · ' : ''}</span>
                  <EditableText label="Location" value={entry.location} placeholder="Location" target={target('location')} onChange={set('location')} onFocus={focus} />
                </>
              )}
              {entryStyle === 'stacked' && (
                <>
                  <span className="cs-sep"> · </span>
                  {certification ? singleDate : dates}
                </>
              )}
            </span>
          );
          const body = (
            <>
              {ENTRY_FIELDS[kind]?.description && (
                <LineEditor className="cs-entry__description" label="Description" value={entry.description} placeholder="Start a line with “- ” for a bullet." target={target('description')} onChange={set('description')} onFocus={focus} />
              )}
              {ENTRY_FIELDS[kind]?.tags && <TagEditor tags={entry.tags} onChange={setTags} placeholder={`Add ${ENTRY_FIELDS[kind]?.tags?.toLowerCase() ?? 'tag'}`} chips={t.skills === 'tags' && !t.plain} label={kind === 'projects' ? 'Tools' : null} />}
              {ENTRY_FIELDS[kind]?.url && (
                <EditableText className="cs-entry__url" label="Link" value={entry.url} placeholder={selected ? 'https://' : ''} target={target('url')} onChange={set('url')} onFocus={focus} />
              )}
            </>
          );
          if (entryStyle === 'dates-left' || entryStyle === 'table' || entryStyle === 'timeline') {
            return wrap(
              <>
                <div className="cs-entry__side">{certification ? singleDate : dates}</div>
                {entryStyle === 'timeline' && <span className="cs-entry__dot" aria-hidden="true" />}
                <div className="cs-entry__main">
                  {title}
                  {subtitle}
                  {body}
                </div>
              </>,
              'cs-entry--columns',
            );
          }
          if (entryStyle === 'dates-right') {
            return wrap(
              <>
                <div className="cs-entry__head">
                  {title}
                  {certification ? singleDate : dates}
                </div>
                {subtitle}
                {body}
              </>,
            );
          }
          return wrap(
            <>
              {title}
              {subtitle}
              {body}
            </>,
          );
        })}
      </div>
    );
  };

  const contact = (side: boolean) => {
    const selected = selection?.kind === 'header';
    const items: { field: 'email' | 'phone' | 'location' | 'website' | 'linkedin' | 'github'; placeholder: string; link?: boolean }[] = [
      { field: 'email', placeholder: 'email@example.com' },
      { field: 'phone', placeholder: '+1 555 000 0000' },
      { field: 'location', placeholder: 'City, Country' },
      { field: 'website', placeholder: 'https://your.site', link: true },
      { field: 'linkedin', placeholder: 'https://linkedin.com/in/…', link: true },
      { field: 'github', placeholder: 'https://github.com/…', link: true },
    ];
    const shown = items.filter((i) => selected || h[i.field]);
    return (
      <div className={side ? 'cs-contact cs-contact--side' : 'cs-contact'}>
        {shown.map((item, i) => (
          <span key={item.field} className="cs-contact__item">
            {i > 0 && !side && <span className="cs-sep">{t.plain ? '  |  ' : ' · '}</span>}
            <EditableText
              className={item.link ? 'cs-contact__link' : ''}
              label={item.field}
              value={item.link && !selected && h[item.field] ? displayUrl(h[item.field]) : h[item.field]}
              placeholder={item.placeholder}
              target={{ scope: 'header', field: item.field }}
              onChange={(value) => edit((d) => ({ ...d, content: { ...d.content, header: { ...d.content.header, [item.field]: value } } }), `header-${item.field}`)}
              onFocus={() => onSelect({ kind: 'header' })}
            />
          </span>
        ))}
        {shown.length === 0 && <span className="cs-contact__hint">Click to add contact details</span>}
      </div>
    );
  };

  const nameField = (
    <EditableText
      as="h1"
      className={`cs-name${t.accentName ? ' cs-name--accent' : ''}`}
      label="Full name"
      value={h.fullName}
      placeholder="Your name"
      target={{ scope: 'header', field: 'fullName' }}
      onChange={(fullName) => edit((d) => ({ ...d, content: { ...d.content, header: { ...d.content.header, fullName } } }), 'header-fullName')}
      onFocus={() => onSelect({ kind: 'header' })}
    />
  );
  const headlineField = (
    <EditableText
      as="p"
      className="cs-headline"
      label="Headline"
      value={h.headline}
      placeholder="Headline, e.g. Senior Data Engineer"
      target={{ scope: 'header', field: 'headline' }}
      onChange={(headline) => edit((d) => ({ ...d, content: { ...d.content, header: { ...d.content.header, headline } } }), 'header-headline')}
      onFocus={() => onSelect({ kind: 'header' })}
    />
  );
  const photo = showPhoto ? <img className={`cs-photo cs-photo--${t.photo ?? 'square'}`} src={photoSrc} alt="" /> : null;
  const initials = h.fullName
    .trim()
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((w) => w[0]?.toUpperCase() ?? '')
    .join('');
  const headerStyle = panel && t.header === 'band' ? 'left' : t.header;
  const header = (
    <header
      className={`cs-header cs-header--${headerStyle}${selection?.kind === 'header' ? ' cs-header--selected' : ''}`}
      onMouseDown={(e) => {
        if ((e.target as HTMLElement).closest('[contenteditable], button')) return;
        onSelect({ kind: 'header' });
      }}
    >
      {headerStyle === 'monogram' && (photo ?? <span className="cs-monogram">{initials || '·'}</span>)}
      {headerStyle !== 'monogram' && photo && <span className="cs-header__photo">{photo}</span>}
      <div className="cs-header__text">
        {nameField}
        {headlineField}
        {headerStyle !== 'split' && !panel && contact(false)}
      </div>
      {headerStyle === 'split' && !panel && <div className="cs-header__aside">{contact(true)}</div>}
    </header>
  );

  const addSectionMenu = (
    <Menu
      align="start"
      placement="above"
      trigger={(props) => (
        <button type="button" className="cs-add-section" disabled={sections.length >= 30} {...props}>
          <PlusIcon /> Add section
        </button>
      )}
    >
      {(close) => (
        <div className="section-menu">
          {SECTION_KINDS.map((k) => (
            <button
              key={k.kind}
              type="button"
              role="menuitem"
              className="menu__item section-menu__item"
              onClick={() => {
                close();
                edit((d) => ({ ...d, content: addSection(d.content, k.kind as SectionKind) }));
              }}
            >
              <span className="section-menu__label">{k.label}</span>
              <span className="section-menu__description">{k.description}</span>
            </button>
          ))}
        </div>
      )}
    </Menu>
  );

  const guides = useMemo(() => {
    const count = Math.max(1, Math.ceil(4));
    return Array.from({ length: count }, (_, i) => i + 1);
  }, []);

  return (
    <div ref={root} className={`cs-root${fontsReady ? ' cs-root--fonts' : ''}`} onMouseDown={(e) => e.target === root.current && onSelect(null)}>
      <div
        className={`cs cs--${layout} cs--header-${headerStyle}${t.plain ? ' cs--plain' : ''}${tinted ? ' cs--tinted' : ''}${sideStyle === 'outlined' ? ' cs--outlined' : ''}${t.sidebar?.side === 'right' ? ' cs--side-right' : ''}`}
        style={{ ...vars, width: `${sheetWidth}px` }}
        data-page-size={draft.pageSize}
      >
        {t.pattern && t.pattern !== 'none' && (tinted || headerStyle === 'band') && <span className={`cs-pattern cs-pattern--${t.pattern}${headerStyle === 'band' && !tinted ? ' cs-pattern--band' : ''}`} aria-hidden="true" />}
        {guides.map((n) => (
          <span key={n} className="cs-page-guide" style={{ top: `${(pageHeight * n).toFixed(1)}px` }} aria-hidden="true">
            <span>Page {n + 1} (approximate)</span>
          </span>
        ))}
        {isLetter ? (
          <LetterBody draft={draft} header={header} edit={edit} onSelect={onSelect} letter={draft.letter ?? {}} centerDate={t.header === 'center'} />
        ) : layout === 'single' ? (
          <div className="cs-main">
            {header}
            {mainSections.map((s, i) => renderSection(s, i, mainSections, false))}
            {sideSections.map((s, i) => renderSection(s, mainSections.length + i, sideSections, false))}
            {addSectionMenu}
          </div>
        ) : (
          <div className="cs-columns">
            <aside className="cs-side" aria-label="Side column">
              {panel && (
                <div className="cs-side__contact">
                  {photo && <span className="cs-side__photo">{photo}</span>}
                  <h2 className="cs-section__title cs-side__contact-title">Contact</h2>
                  {contact(true)}
                </div>
              )}
              {sideSections.map((s, i) => renderSection(s, i, sideSections, true))}
              {sideSections.length === 0 && <p className="cs-side__empty">Skills, languages, links and certifications appear here.</p>}
            </aside>
            <div className="cs-main">
              {panel ? (
                <header className={`cs-header cs-header--${headerStyle}${selection?.kind === 'header' ? ' cs-header--selected' : ''}`} onMouseDown={() => onSelect({ kind: 'header' })}>
                  {headerStyle === 'monogram' && <span className="cs-monogram">{initials || '·'}</span>}
                  <div className="cs-header__text">
                    {nameField}
                    {headlineField}
                  </div>
                </header>
              ) : (
                header
              )}
              {mainSections.map((s, i) => renderSection(s, i, mainSections, false))}
              {addSectionMenu}
            </div>
          </div>
        )}
      </div>
      {toolbar && (
        <div className="cs-float" style={{ left: toolbar.x, top: toolbar.y }} role="toolbar" aria-label="Selection">
          {isRichTarget(toolbar.selection.target) && (
            <>
              <button type="button" className="cs-float__button" title="Bold" aria-label="Bold" onMouseDown={(e) => e.preventDefault()} onClick={() => format('bold')}>
                <b>B</b>
              </button>
              <button type="button" className="cs-float__button" title="Italic" aria-label="Italic" onMouseDown={(e) => e.preventDefault()} onClick={() => format('italic')}>
                <i>I</i>
              </button>
              <button type="button" className="cs-float__button" title="Link" aria-label="Link" onMouseDown={(e) => e.preventDefault()} onClick={() => setLinking((l) => (l === null ? 'https://' : null))}>
                <LinkIcon />
              </button>
            </>
          )}
          <button type="button" className="cs-float__button cs-float__button--ai" onMouseDown={(e) => e.preventDefault()} onClick={() => onAskAi(toolbar.selection)}>
            <SparkleIcon /> AI
          </button>
          {linking !== null && (
            <form
              className="cs-float__link"
              onSubmit={(e) => {
                e.preventDefault();
                if (/^(https?:\/\/|mailto:)/.test(linking)) format('createLink', linking);
                setLinking(null);
              }}
            >
              <input className="input input--small" aria-label="Link URL" value={linking} autoFocus onChange={(e) => setLinking(e.target.value)} onMouseDown={(e) => e.stopPropagation()} />
              <button type="submit" className="button button--primary button--small">
                Link
              </button>
            </form>
          )}
        </div>
      )}
    </div>
  );
}

function TagEditor({ tags, onChange, placeholder, chips, list, label }: { tags: string[]; onChange: (tags: string[]) => void; placeholder: string; chips: boolean; list?: boolean; label?: string | null }) {
  const [draft, setDraft] = useState('');
  const add = () => {
    const value = draft.trim();
    if (!value) return;
    if (!tags.some((t) => t.toLowerCase() === value.toLowerCase())) onChange([...tags, value]);
    setDraft('');
  };
  return (
    <span className={`cs-tags${chips ? ' cs-tags--chips' : ''}${list ? ' cs-tags--list' : ''}`}>
      {label && tags.length > 0 && <span className="cs-tags__label">{label}: </span>}
      {tags.map((tag, i) => (
        <span key={`${tag}-${i}`} className="cs-tag">
          {tag}
          <button type="button" className="cs-tag__remove" aria-label={`Remove ${tag}`} onClick={() => onChange(tags.filter((_, j) => j !== i))}>
            ×
          </button>
          {!chips && !list && i < tags.length - 1 && <span className="cs-sep"> · </span>}
        </span>
      ))}
      <input
        className="cs-tags__input"
        aria-label={placeholder}
        placeholder={tags.length ? '+' : placeholder}
        value={draft}
        size={Math.max(2, draft.length + 1)}
        onChange={(e) => setDraft(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter' || e.key === ',') {
            e.preventDefault();
            add();
          }
          if (e.key === 'Backspace' && !draft && tags.length) onChange(tags.slice(0, -1));
        }}
        onBlur={add}
      />
    </span>
  );
}

function LetterBody({
  draft,
  header,
  edit,
  onSelect,
  letter,
  centerDate,
}: {
  draft: PortfolioInput;
  header: ReactNode;
  edit: DocumentCanvasProps['edit'];
  onSelect: (s: CanvasSelection) => void;
  letter: CoverLetter;
  centerDate: boolean;
}) {
  const set = (field: keyof CoverLetter) => (value: string) => edit((d) => ({ ...d, letter: { ...(d.letter ?? {}), [field]: value } }), `letter-${field}`);
  const target = (field: keyof CoverLetter): FieldTarget => ({ scope: 'letter', field });
  const focus = () => onSelect(null);
  const field = (name: keyof CoverLetter, label: string, placeholder: string, className = '') => (
    <EditableText className={`cs-letter__field ${className}`} label={label} value={letter[name] ?? ''} placeholder={placeholder} target={target(name)} onChange={set(name)} onFocus={focus} />
  );
  void draft;
  return (
    <div className="cs-main cs-letter">
      {header}
      <div className={centerDate ? 'cs-letter__date cs-letter__date--right' : 'cs-letter__date'}>{field('date', 'Date', formatDate(new Date().toISOString().slice(0, 7)))}</div>
      <div className="cs-letter__recipient">
        {field('recipientName', 'Recipient name', 'Recipient name', 'cs-letter__recipient-name')}
        {field('recipientTitle', 'Recipient title', 'Recipient title')}
        {field('company', 'Company', 'Company')}
        <EditableText className="cs-letter__field" multiline label="Address" value={letter.address ?? ''} placeholder={'Street\nCity'} target={target('address')} onChange={set('address')} onFocus={focus} />
      </div>
      {field('subject', 'Subject', 'Subject, e.g. Application for …', 'cs-letter__subject')}
      {field('greeting', 'Greeting', 'Dear …,', 'cs-letter__greeting')}
      <LineEditor className="cs-letter__body" paragraphs label="Letter body" value={letter.body ?? ''} placeholder="Write the letter. Blank lines separate paragraphs." target={target('body')} onChange={set('body')} onFocus={focus} />
      {field('closing', 'Closing', 'Kind regards,', 'cs-letter__closing')}
      {field('signature', 'Signature', 'Your name', 'cs-letter__signature')}
      <span className="cs-letter__position-note">
        Position: {field('position', 'Position', 'Position applied for')}
      </span>
    </div>
  );
}

