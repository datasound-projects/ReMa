import type { CoverLetter, PageSize, PortfolioContent, PortfolioKind, PortfolioStyle } from '../../services/portfolioService';
import type { LayoutInput } from './layout';
import { SAMPLE_CONTENT, SAMPLE_LETTER } from './sample';
import { templateById } from './templates';

const cache = new Map<string, Promise<string>>();
// One render at a time keeps the interface responsive.
let queue: Promise<unknown> = Promise.resolve();

/** The sample document for a template (a CV or a letter, by the template's kind). */
export function sampleInput(templateId: string, pageSize: PageSize): LayoutInput {
  const template = templateById(templateId);
  return template.kind === 'letter'
    ? { name: 'Sample', kind: 'cover_letter', templateId: template.id, pageSize, accent: '', content: SAMPLE_CONTENT, letter: SAMPLE_LETTER }
    : { name: 'Sample', kind: 'cv', templateId: template.id, pageSize, accent: '', content: SAMPLE_CONTENT };
}

/**
 * A picture of a template's first page with sample content (rendered by
 * the same engine as real documents), cached for the session.
 */
export function templateThumbnail(templateId: string, pageSize: PageSize, width = 240): Promise<string> {
  const key = `${templateId}:${pageSize}:${width}`;
  const cached = cache.get(key);
  if (cached) return cached;
  const next = queue.then(async () => {
    const [{ renderPdf }, { firstPageImage }] = await Promise.all([import('./pdf'), import('../pdf/thumbnail')]);
    const bytes = await renderPdf(sampleInput(templateId, pageSize));
    return firstPageImage(bytes, width);
  });
  queue = next.catch(() => undefined);
  cache.set(key, next);
  next.catch(() => cache.delete(key));
  return next;
}

const documents = new Map<string, Promise<string>>();

/**
 * A picture of a document's own first page, cached by `key` (the document
 * id and its last update), rendered one at a time like the templates.
 */
export function documentThumbnail(key: string, input: LayoutInput, width = 240): Promise<string> {
  const cacheKey = `${key}:${width}`;
  const cached = documents.get(cacheKey);
  if (cached) return cached;
  const next = queue.then(async () => {
    const [{ renderPdf }, { firstPageImage }] = await Promise.all([import('./pdf'), import('../pdf/thumbnail')]);
    const bytes = await renderPdf(input);
    return firstPageImage(bytes, width);
  });
  queue = next.catch(() => undefined);
  if (documents.size > 60) documents.clear();
  documents.set(cacheKey, next);
  next.catch(() => documents.delete(cacheKey));
  return next;
}

/** The layout input of a stored document (its own content and design). */
export function documentInput(doc: {
  name: string;
  kind?: PortfolioKind;
  templateId: string;
  pageSize: PageSize;
  accent: string;
  style?: PortfolioStyle;
  content: PortfolioContent;
  letter?: CoverLetter;
}): LayoutInput {
  return {
    name: doc.name,
    kind: doc.kind ?? 'cv',
    templateId: doc.templateId,
    pageSize: doc.pageSize,
    accent: doc.accent,
    style: doc.style ?? null,
    content: doc.content,
    letter: doc.letter ?? null,
  };
}
