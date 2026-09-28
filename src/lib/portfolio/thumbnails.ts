import type { PageSize } from '../../services/portfolioService';
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
