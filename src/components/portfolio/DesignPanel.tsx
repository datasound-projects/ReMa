import { useRef, useState } from 'react';

import { PAGE_SIZES } from '../../lib/portfolio/layout';
import { photoFromFile } from '../../lib/portfolio/photo';
import {
  COLUMN_LAYOUTS,
  DIVIDERS,
  FONT_PAIRINGS,
  HEADERS,
  MARGINS,
  PALETTES,
  PATTERNS,
  SCALES,
  SIDEBARS,
  SPACINGS,
  isDefaultStyle,
  normalizeStyle,
} from '../../lib/portfolio/style';
import { ACCENTS, templateById, type Template } from '../../lib/portfolio/templates';
import type { PageSize, PortfolioInput, PortfolioStyle } from '../../services/portfolioService';
import { CheckIcon, EyeIcon } from '../icons';
import { TemplateGallery } from './TemplateGallery';

interface DesignPanelProps {
  draft: PortfolioInput;
  edit: (update: (d: PortfolioInput) => PortfolioInput, group?: string) => void;
  onPreviewTemplate: (template: Template) => void;
}

/** Template, paper, palettes, fonts, spacing, layout and decoration. */
export function DesignPanel({ draft, edit, onPreviewTemplate }: DesignPanelProps) {
  const isLetter = (draft.kind ?? 'cv') === 'cover_letter';
  const template = templateById(draft.templateId, isLetter ? 'letter' : 'cv');
  const style = normalizeStyle(draft.style);
  const [photoError, setPhotoError] = useState<string | null>(null);
  const [customOpen, setCustomOpen] = useState(false);
  const fileInput = useRef<HTMLInputElement>(null);
  const plain = !!template.plain;

  const setStyle = (patch: Partial<PortfolioStyle>, group?: string) => edit((d) => ({ ...d, style: { ...normalizeStyle(d.style), ...patch } }), group);
  const reset = () => edit((d) => ({ ...d, accent: '', style: {} }));

  const choosePhoto = async (file: File | undefined) => {
    if (!file) return;
    setPhotoError(null);
    try {
      const photo = await photoFromFile(file);
      edit((d) => ({ ...d, content: { ...d.content, header: { ...d.content.header, photo } }, style: { ...normalizeStyle(d.style), showPhoto: true } }));
    } catch (error) {
      setPhotoError(error instanceof Error ? error.message : 'The image could not be used.');
    }
  };

  const choice = (label: string, value: string, options: readonly { value: string; label: string }[], onChange: (v: string) => void, allowDefault = true) => (
    <div className="design__row">
      <span className="design__label">{label}</span>
      <div className="segmented segmented--wrap" role="radiogroup" aria-label={label}>
        {allowDefault && (
          <button type="button" role="radio" aria-checked={value === ''} className={value === '' ? 'segmented__option segmented__option--active' : 'segmented__option'} onClick={() => onChange('')}>
            Template
          </button>
        )}
        {options.map((o) => (
          <button key={o.value} type="button" role="radio" aria-checked={value === o.value} className={value === o.value ? 'segmented__option segmented__option--active' : 'segmented__option'} onClick={() => onChange(o.value)}>
            {o.label}
          </button>
        ))}
      </div>
    </div>
  );

  const colorInput = (label: string, field: 'headingColor' | 'textColor' | 'backgroundColor' | 'panelColor', fallback: string) => (
    <label className="design__color">
      <span>{label}</span>
      <input type="color" value={style[field] || fallback} aria-label={label} onChange={(e) => setStyle({ [field]: e.target.value }, `color-${field}`)} />
      {style[field] && (
        <button type="button" className="link-button" onClick={() => setStyle({ [field]: '' })}>
          Clear
        </button>
      )}
    </label>
  );

  return (
    <div className="design">
      <section className="design__group">
        <div className="design__head">
          <h3 className="design__title">Template</h3>
          <button type="button" className="button button--ghost button--small" onClick={() => onPreviewTemplate(template)}>
            <EyeIcon className="button__icon" />
            Preview with my content
          </button>
        </div>
        <TemplateGallery
          kind={isLetter ? 'letter' : 'cv'}
          pageSize={draft.pageSize}
          compact
          selectedId={template.id}
          onSelect={(t) => edit((d) => ({ ...d, templateId: t.id }))}
          onPreview={onPreviewTemplate}
        />
        <p className="form__hint">Switching keeps your content. Preview any template with your own text before applying it.</p>
      </section>

      <section className="design__group">
        <h3 className="design__title">Paper</h3>
        <div className="segmented" role="radiogroup" aria-label="Paper size">
          {(['a4', 'letter'] as PageSize[]).map((size) => (
            <button key={size} type="button" role="radio" aria-checked={draft.pageSize === size} className={draft.pageSize === size ? 'segmented__option segmented__option--active' : 'segmented__option'} onClick={() => edit((d) => ({ ...d, pageSize: size }))}>
              {PAGE_SIZES[size].label}
            </button>
          ))}
        </div>
      </section>

      {plain ? (
        <p className="form__hint">This text-first template keeps black text on white; only spacing can change.</p>
      ) : (
        <section className="design__group">
          <h3 className="design__title">Colors</h3>
          <div className="design__row">
            <span className="design__label">Palette</span>
            <div className="palettes" role="radiogroup" aria-label="Palette">
              <button type="button" role="radio" aria-checked={style.palette === ''} className={style.palette === '' ? 'palette palette--active' : 'palette'} onClick={() => setStyle({ palette: '' })} title="Template colors">
                <span className="palette__swatches">
                  <span style={{ background: template.colors.accent }} />
                  <span style={{ background: template.colors.text }} />
                  <span style={{ background: template.colors.panel ?? '#f3f4f6' }} />
                </span>
                <span className="palette__name">Template</span>
              </button>
              {PALETTES.map((p) => (
                <button key={p.id} type="button" role="radio" aria-checked={style.palette === p.id} className={style.palette === p.id ? 'palette palette--active' : 'palette'} onClick={() => setStyle({ palette: p.id })} title={p.name}>
                  <span className="palette__swatches">
                    <span style={{ background: p.accent }} />
                    <span style={{ background: p.heading }} />
                    <span style={{ background: p.panel }} />
                  </span>
                  <span className="palette__name">{p.name}</span>
                </button>
              ))}
            </div>
          </div>
          <div className="design__row">
            <span className="design__label">Accent</span>
            <div className="swatches__grid swatches__grid--inline" role="radiogroup" aria-label="Accent color">
              <button type="button" role="radio" aria-checked={draft.accent === ''} className="swatches__option swatches__option--default" title="Template accent" style={{ background: template.colors.accent }} onClick={() => edit((d) => ({ ...d, accent: '' }))}>
                {draft.accent === '' && <CheckIcon />}
              </button>
              {ACCENTS.map((a) => (
                <button key={a.value} type="button" role="radio" aria-checked={draft.accent === a.value} aria-label={a.label} title={a.label} className="swatches__option" style={{ background: a.value }} onClick={() => edit((d) => ({ ...d, accent: a.value }))}>
                  {draft.accent === a.value && <CheckIcon />}
                </button>
              ))}
              <label className="swatches__custom" title="Custom accent">
                <input type="color" value={draft.accent || template.colors.accent} aria-label="Custom accent color" onChange={(e) => edit((d) => ({ ...d, accent: e.target.value }), 'accent')} />
              </label>
            </div>
          </div>
          <button type="button" className="link-button design__more" aria-expanded={customOpen} onClick={() => setCustomOpen(!customOpen)}>
            {customOpen ? 'Hide custom colors' : 'Custom colors…'}
          </button>
          {customOpen && (
            <div className="design__colors">
              {colorInput('Headings', 'headingColor', template.colors.heading ?? template.colors.text)}
              {colorInput('Text', 'textColor', template.colors.text)}
              {colorInput('Background', 'backgroundColor', template.colors.background ?? '#ffffff')}
              {colorInput('Side panel', 'panelColor', template.colors.panel ?? '#f3f4f6')}
              <p className="form__hint">Colors that would not be readable fall back to the template’s.</p>
            </div>
          )}
        </section>
      )}

      <section className="design__group">
        <h3 className="design__title">Typography</h3>
        <div className="design__row">
          <span className="design__label">Fonts</span>
          <div className="pairings" role="radiogroup" aria-label="Font pairing">
            <button type="button" role="radio" aria-checked={style.fontPairing === ''} className={style.fontPairing === '' ? 'pairing pairing--active' : 'pairing'} onClick={() => setStyle({ fontPairing: '' })}>
              <span className="pairing__name">Template</span>
              <span className="pairing__sample">{template.fonts.heading} + {template.fonts.body}</span>
            </button>
            {FONT_PAIRINGS.map((p) => (
              <button key={p.id} type="button" role="radio" aria-checked={style.fontPairing === p.id} className={style.fontPairing === p.id ? 'pairing pairing--active' : 'pairing'} onClick={() => setStyle({ fontPairing: p.id })}>
                <span className="pairing__name">{p.name}</span>
                <span className="pairing__sample">{p.sample}</span>
              </button>
            ))}
          </div>
        </div>
        {choice('Text size', style.scale ? String(style.scale) : '', SCALES.map((s) => ({ value: String(s.value), label: s.label })), (v) => setStyle({ scale: v ? Number(v) : 0 }))}
        {choice('Line spacing', style.lineSpacing, SPACINGS, (v) => setStyle({ lineSpacing: v }))}
        {!plain && choice('Alignment', style.textAlign, [{ value: 'left', label: 'Left' }, { value: 'justify', label: 'Justified' }], (v) => setStyle({ textAlign: v }))}
      </section>

      <section className="design__group">
        <h3 className="design__title">Spacing</h3>
        {choice('Margins', style.margins, MARGINS, (v) => setStyle({ margins: v }))}
        {choice('Section spacing', style.sectionSpacing, SPACINGS, (v) => setStyle({ sectionSpacing: v }))}
      </section>

      {!plain && !isLetter && (
        <section className="design__group">
          <h3 className="design__title">Layout</h3>
          {choice('Columns', style.columns, COLUMN_LAYOUTS, (v) => setStyle({ columns: v }))}
          {choice('Header', style.header, HEADERS, (v) => setStyle({ header: v }))}
          {(style.columns.startsWith('sidebar') || (style.columns === '' && template.layout === 'sidebar')) && choice('Side panel', style.sidebar, SIDEBARS, (v) => setStyle({ sidebar: v }))}
        </section>
      )}
      {!plain && isLetter && (
        <section className="design__group">
          <h3 className="design__title">Layout</h3>
          {choice('Header', style.header, HEADERS, (v) => setStyle({ header: v }))}
        </section>
      )}

      {!plain && (
        <section className="design__group">
          <h3 className="design__title">Decoration</h3>
          {choice('Dividers', style.dividers, DIVIDERS, (v) => setStyle({ dividers: v }))}
          {choice('Pattern', style.pattern, PATTERNS, (v) => setStyle({ pattern: v }))}
          <p className="form__hint">Patterns are drawn only behind color bands and side panels, never behind body text.</p>
        </section>
      )}

      {!plain && (
        <section className="design__group">
          <h3 className="design__title">Photo</h3>
          <div className="design__photo">
            {draft.content.header.photo ? <img className="design__photo-preview" src={draft.content.header.photo} alt="" /> : <span className="design__photo-empty">No photo</span>}
            <div className="design__photo-actions">
              <input ref={fileInput} type="file" accept="image/png,image/jpeg,image/webp" hidden onChange={(e) => void choosePhoto(e.target.files?.[0])} />
              <button type="button" className="button button--secondary button--small" onClick={() => fileInput.current?.click()}>
                {draft.content.header.photo ? 'Replace…' : 'Add photo…'}
              </button>
              {draft.content.header.photo && (
                <>
                  <label className="checkbox">
                    <input type="checkbox" checked={style.showPhoto} onChange={(e) => setStyle({ showPhoto: e.target.checked })} />
                    <span>Show in this template</span>
                  </label>
                  <button type="button" className="link-button" onClick={() => edit((d) => ({ ...d, content: { ...d.content, header: { ...d.content.header, photo: '' } }, style: { ...normalizeStyle(d.style), showPhoto: false } }))}>
                    Remove
                  </button>
                </>
              )}
            </div>
          </div>
          {photoError && <p className="form-error">{photoError}</p>}
          <p className="form__hint">
            {template.photo ? `This template places the photo as a ${template.photo}.` : 'This template does not place a photo; it is kept for templates that do.'}
          </p>
        </section>
      )}

      <div className="design__reset">
        <button type="button" className="button button--secondary" disabled={isDefaultStyle(draft.style) && draft.accent === ''} onClick={reset}>
          Reset Style
        </button>
        <span className="form__hint">Back to the template’s own colors, fonts and spacing.</span>
      </div>
    </div>
  );
}
