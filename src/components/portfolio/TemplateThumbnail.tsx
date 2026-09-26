import { useEffect, useState } from 'react';

import { templateThumbnail } from '../../lib/portfolio/thumbnails';
import type { PageSize } from '../../services/portfolioService';

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

  return (
    <span className={`template-thumb template-thumb--${pageSize}`} aria-hidden={src ? undefined : true}>
      {src ? (
        <img src={src} alt={`${name} template preview`} draggable={false} />
      ) : (
        <span className={failed ? 'template-thumb__placeholder' : 'template-thumb__placeholder template-thumb__placeholder--loading'}>
          {failed ? 'Preview unavailable' : ''}
        </span>
      )}
    </span>
  );
}
