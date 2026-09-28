import { useMemo, useState } from 'react';

import { CATEGORIES, CV_TEMPLATES, LETTER_TEMPLATES, type StyleTag, type Template } from '../../lib/portfolio/templates';
import type { PageSize } from '../../services/portfolioService';
import { EyeIcon } from '../icons';
import { TemplateThumbnail } from './TemplateThumbnail';

const STYLE_TAGS: { tag: StyleTag; label: string }[] = [
  { tag: 'single-column', label: 'Single column' },
  { tag: 'two-column', label: 'Two columns' },
  { tag: 'sidebar', label: 'Side panel' },
  { tag: 'minimal', label: 'Minimal' },
  { tag: 'editorial', label: 'Editorial' },
  { tag: 'executive', label: 'Executive' },
  { tag: 'modern', label: 'Modern' },
  { tag: 'classic', label: 'Classic' },
  { tag: 'compact', label: 'Compact' },
  { tag: 'text-first', label: 'Text-first' },
  { tag: 'photo', label: 'With photo' },
];

interface TemplateGalleryProps {
  kind: 'cv' | 'letter';
  pageSize?: PageSize;
  /** The selected template (in a picker). */
  selectedId?: string;
  /** Compact: smaller cards, no descriptions (inside dialogs and panels). */
  compact?: boolean;
  onSelect: (template: Template) => void;
  onPreview?: (template: Template) => void;
  /** Label of the card's main action; the whole card selects. */
  actionLabel?: string;
}

/** Templates of one kind with category and style filters. */
export function TemplateGallery({ kind, pageSize = 'a4', selectedId, compact, onSelect, onPreview, actionLabel }: TemplateGalleryProps) {
  const [category, setCategory] = useState<string>('all');
  const [tag, setTag] = useState<StyleTag | 'all'>('all');
  const all = kind === 'letter' ? LETTER_TEMPLATES : CV_TEMPLATES;
  const shown = useMemo(
    () => all.filter((t) => (category === 'all' || t.category === category) && (tag === 'all' || t.tags.includes(tag))),
    [all, category, tag],
  );
  const usedTags = useMemo(() => STYLE_TAGS.filter((s) => all.some((t) => t.tags.includes(s.tag))), [all]);

  return (
    <div className={compact ? 'gallery gallery--compact' : 'gallery'}>
      <div className="gallery__filters">
        {kind === 'cv' && (
          <label className="gallery__filter">
            <span className="visually-hidden">Category</span>
            <select className="input input--small gallery__select" value={category} onChange={(e) => setCategory(e.target.value)} aria-label="Category">
              <option value="all">All categories</option>
              {CATEGORIES.map((c) => (
                <option key={c} value={c}>
                  {c}
                </option>
              ))}
            </select>
          </label>
        )}
        <div className="gallery__tags" role="radiogroup" aria-label="Style">
          <button
            type="button"
            role="radio"
            aria-checked={tag === 'all'}
            className={tag === 'all' ? 'filter-chip filter-chip--active' : 'filter-chip'}
            onClick={() => setTag('all')}
          >
            All styles
          </button>
          {usedTags.map((s) => (
            <button
              key={s.tag}
              type="button"
              role="radio"
              aria-checked={tag === s.tag}
              className={tag === s.tag ? 'filter-chip filter-chip--active' : 'filter-chip'}
              onClick={() => setTag(tag === s.tag ? 'all' : s.tag)}
            >
              {s.label}
            </button>
          ))}
        </div>
        <span className="gallery__count">
          {shown.length} of {all.length}
        </span>
      </div>
      {shown.length === 0 ? (
        <p className="gallery__empty">No template matches these filters.</p>
      ) : (
        <div className="gallery__grid" role={selectedId !== undefined ? 'radiogroup' : undefined}>
          {shown.map((t) => {
            const selected = t.id === selectedId;
            return (
              <div key={t.id} className={selected ? 'template-card template-card--selected' : 'template-card'}>
                <button
                  type="button"
                  className="template-card__pick"
                  role={selectedId !== undefined ? 'radio' : undefined}
                  aria-checked={selectedId !== undefined ? selected : undefined}
                  aria-label={selectedId !== undefined ? t.name : `${actionLabel ?? 'Use'} ${t.name}`}
                  onClick={() => onSelect(t)}
                >
                  <TemplateThumbnail templateId={t.id} pageSize={pageSize} name={t.name} />
                </button>
                <div className="template-card__text">
                  <span className="template-card__name">{t.name}</span>
                  {!compact && <span className="template-card__category">{kind === 'letter' ? t.tags.join(' · ') : t.category}</span>}
                  {!compact && <span className="template-card__description">{t.description}</span>}
                </div>
                <div className="template-card__actions">
                  {onPreview && (
                    <button type="button" className="button button--ghost button--small" onClick={() => onPreview(t)}>
                      <EyeIcon className="button__icon" />
                      Preview
                    </button>
                  )}
                  {actionLabel && (
                    <button type="button" className="button button--secondary button--small" onClick={() => onSelect(t)}>
                      {actionLabel}
                    </button>
                  )}
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
