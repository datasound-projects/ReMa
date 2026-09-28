import { useEffect, useState } from 'react';

import type { LayoutInput } from '../../lib/portfolio/layout';
import { documentThumbnail, templateThumbnail } from '../../lib/portfolio/thumbnails';
import type { PageSize } from '../../services/portfolioService';

function Thumb({ src, failed, pageSize, alt }: { src: string | null; failed: boolean; pageSize: PageSize; alt: string }) {
  return (
    <span className={`template-thumb template-thumb--${pageSize}`} aria-hidden={src ? undefined : true}>
      {src ? (
        <img src={src} alt={alt} draggable={false} />
      ) : (
        <span className={failed ? 'template-thumb__placeholder' : 'template-thumb__placeholder template-thumb__placeholder--loading'}>
          {failed ? 'Preview unavailable' : ''}
        </span>
      )}
    </span>
  );
}

/** A rendered first page of a template with sample content. */
export function TemplateThumbnail({ templateId, pageSize = 'a4', name }: { templateId: string; pageSize?: PageSize; name: string }) {
  const [src, setSrc] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    let active = true;
    templateThumbnail(templateId, pageSize)
      .then((url) => active && setSrc(url))
      .catch(() => active && setFailed(true));
    return () => {
      active = false;
    };
  }, [templateId, pageSize]);
  return <Thumb src={src} failed={failed} pageSize={pageSize} alt={`${name} template preview`} />;
}

/** A rendered first page of a document's own content (re-rendered when `cacheKey` changes). */
export function DocumentThumbnail({ cacheKey, input, name }: { cacheKey: string; input: LayoutInput; name: string }) {
  const [src, setSrc] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    let active = true;
    documentThumbnail(cacheKey, input)
      .then((url) => active && setSrc(url))
      .catch(() => active && setFailed(true));
    return () => {
      active = false;
    };
  }, [cacheKey, input]);
  return <Thumb src={src} failed={failed} pageSize={input.pageSize} alt={`Preview of ${name}`} />;
}
