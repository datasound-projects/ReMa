import type { PageSize } from '../../services/portfolioService';
import { SAMPLE_CONTENT } from './sample';

const cache = new Map<string, Promise<string>>();
// One render at a time keeps the interface responsive.
let queue: Promise<unknown> = Promise.resolve();

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
    const bytes = await renderPdf({ name: 'Sample', templateId, pageSize, accent: '', content: SAMPLE_CONTENT });
    return firstPageImage(bytes, width);
  });
  queue = next.catch(() => undefined);
  cache.set(key, next);
  next.catch(() => cache.delete(key));
  return next;
}
