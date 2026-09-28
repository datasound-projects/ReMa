/**
 * Addressing one text field of a document (for the canvas, the selection
 * toolbar and AI proposals): where it is and how to read or write it.
 */
import type { CoverLetter, PortfolioEntry, PortfolioHeader, PortfolioInput } from '../../services/portfolioService';
import { stripMarkup } from './layout';

export type FieldTarget =
  | { scope: 'name' }
  | { scope: 'header'; field: Exclude<keyof PortfolioHeader, 'photo'> }
  | { scope: 'section'; sectionId: string; field: 'title' | 'text' }
  | { scope: 'entry'; sectionId: string; entryId: string; field: Exclude<keyof PortfolioEntry, 'id' | 'tags'> }
  | { scope: 'letter'; field: keyof CoverLetter };

/** Fields whose text may carry inline markup and bullets. */
export function isRichTarget(target: FieldTarget): boolean {
  return (
    (target.scope === 'section' && target.field === 'text') ||
    (target.scope === 'entry' && target.field === 'description') ||
    (target.scope === 'letter' && target.field === 'body')
  );
}

export function targetKey(target: FieldTarget): string {
  switch (target.scope) {
    case 'name':
      return 'name';
    case 'header':
      return `header.${target.field}`;
    case 'section':
      return `section.${target.sectionId}.${target.field}`;
    case 'entry':
      return `entry.${target.sectionId}.${target.entryId}.${target.field}`;
    case 'letter':
      return `letter.${target.field}`;
  }
}

export function getField(draft: PortfolioInput, target: FieldTarget): string {
  switch (target.scope) {
    case 'name':
      return draft.name;
    case 'header':
      return draft.content.header[target.field] ?? '';
    case 'section':
      return draft.content.sections.find((s) => s.id === target.sectionId)?.[target.field] ?? '';
    case 'entry':
      return draft.content.sections.find((s) => s.id === target.sectionId)?.entries.find((e) => e.id === target.entryId)?.[target.field] ?? '';
    case 'letter':
      return draft.letter?.[target.field] ?? '';
  }
}

export function setField(draft: PortfolioInput, target: FieldTarget, value: string): PortfolioInput {
  switch (target.scope) {
    case 'name':
      return { ...draft, name: value };
    case 'header':
      return { ...draft, content: { ...draft.content, header: { ...draft.content.header, [target.field]: value } } };
    case 'section':
      return {
        ...draft,
        content: {
          ...draft.content,
          sections: draft.content.sections.map((s) => (s.id === target.sectionId ? { ...s, [target.field]: value } : s)),
        },
      };
    case 'entry':
      return {
        ...draft,
        content: {
          ...draft.content,
          sections: draft.content.sections.map((s) =>
            s.id === target.sectionId
              ? { ...s, entries: s.entries.map((e) => (e.id === target.entryId ? { ...e, [target.field]: value } : e)) }
              : s,
          ),
        },
      };
    case 'letter':
      return { ...draft, letter: { ...(draft.letter ?? {}), [target.field]: value } };
  }
}

/**
 * Replaces selected text inside a field. The selection is what the user
 * saw (without markup); it is matched in the raw value first, then in the
 * plain text (dropping that field's markup), else the whole field changes.
 */
export function replaceSelection(value: string, selected: string, replacement: string): string {
  const s = selected.trim();
  if (!s) return replacement;
  if (value.includes(s)) return value.replace(s, replacement);
  const plain = stripMarkup(value);
  if (plain.includes(s)) return plain.replace(s, replacement);
  const loose = s.replace(/\s+/g, ' ');
  const plainLoose = plain.replace(/\s+/g, ' ');
  if (plainLoose.includes(loose)) return plainLoose.replace(loose, replacement);
  return replacement;
}
