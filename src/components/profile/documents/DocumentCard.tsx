import { useState } from 'react';

import { useAction } from '../../../hooks/useAction';
import { formatDateTime } from '../../../lib/format';
import { FORMAT_LABELS, formatSize } from '../../../lib/documents';
import {
  deleteProfileDocument,
  replaceProfileDocument,
  setPrimaryDocument,
  updateProfileDocument,
  type ProfileDocument,
} from '../../../services/profileService';
import { FileIcon, MoreIcon, StarIcon } from '../../icons';
import { Menu, type MenuItem } from '../../ui/Menu';

interface DocumentCardProps {
  document: ProfileDocument;
  onPreview: (document: ProfileDocument) => void;
  /** Extra menu entries (e.g. "Use as CV" for other documents). */
  extraItems?: MenuItem[];
  /** CVs can be marked primary. */
  canBePrimary?: boolean;
}

/**
 * One stored file: name, type, size and date, with preview and the
 * actions that manage it. Everything happens in place.
 */
export function DocumentCard({ document: doc, onPreview, extraItems = [], canBePrimary = false }: DocumentCardProps) {
  const action = useAction();
  const [name, setName] = useState<string | null>(null);
  const [confirming, setConfirming] = useState(false);

  const rename = () => {
    const next = name?.trim();
    setName(null);
    if (next && next !== doc.name) void action.run(() => updateProfileDocument(doc.id, next, doc.kind));
  };

  const items: MenuItem[] = [
    { label: 'Rename', onSelect: () => setName(doc.name) },
    { label: 'Replace file…', onSelect: () => void action.run(() => replaceProfileDocument(doc.id)) },
    ...(canBePrimary && !doc.isPrimary
      ? [{ label: 'Mark as primary', onSelect: () => void action.run(() => setPrimaryDocument(doc.id)) }]
      : []),
    ...extraItems,
    { label: 'Remove', danger: true, onSelect: () => setConfirming(true) },
  ];

  return (
    <article className={doc.isPrimary ? 'doc-card doc-card--primary' : 'doc-card'} aria-label={doc.name}>
      <button
        type="button"
        className={`doc-card__thumb doc-card__thumb--${doc.format}`}
        onClick={() => onPreview(doc)}
        aria-label={`Preview ${doc.name}`}
      >
        <FileIcon className="doc-card__icon" />
        <span className="doc-card__format">{FORMAT_LABELS[doc.format]}</span>
      </button>
      <div className="doc-card__main">
        <div className="doc-card__title-row">
          {name === null ? (
            <button type="button" className="doc-card__name" title="Preview" onClick={() => onPreview(doc)}>
              {doc.name}
            </button>
          ) : (
            <input
              className="input input--small doc-card__rename"
              aria-label="Document name"
              autoFocus
              maxLength={200}
              value={name}
              onChange={(e) => setName(e.target.value)}
              onBlur={rename}
              onKeyDown={(e) => {
                if (e.key === 'Enter') rename();
                if (e.key === 'Escape') setName(null);
              }}
            />
          )}
          {doc.isPrimary && (
            <span className="badge badge--brand doc-card__badge" title="Comes first in Profile context">
              <StarIcon aria-hidden="true" />
              Primary
            </span>
          )}
        </div>
        <span className="doc-card__meta">
          {FORMAT_LABELS[doc.format]} · {formatSize(doc.size)} · added {formatDateTime(doc.createdAt)}
        </span>
        {doc.originalName !== doc.name && (
          <span className="doc-card__file" title="File name as uploaded">
            {doc.originalName}
          </span>
        )}
        {!doc.hasText && (doc.format === 'pdf' || doc.format === 'docx') && (
          <span className="doc-card__warning">No readable text: Chat can only mention it by name.</span>
        )}
        {confirming && (
          <div className="doc-card__confirm" role="group" aria-label="Confirm removal">
            <span>Remove this file from ReMa?</span>
            <button
              type="button"
              className="button button--danger button--small"
              disabled={action.busy}
              onClick={() => void action.run(() => deleteProfileDocument(doc.id))}
            >
              Remove
            </button>
            <button type="button" className="button button--ghost button--small" onClick={() => setConfirming(false)}>
              Cancel
            </button>
          </div>
        )}
        {action.error && <p className="form-error">{action.error}</p>}
      </div>
      <div className="doc-card__actions">
        <button type="button" className="button button--secondary button--small" onClick={() => onPreview(doc)}>
          Preview
        </button>
        <Menu
          items={items}
          trigger={(props) => (
            <button type="button" className="icon-button icon-button--small" aria-label="More actions" title="More actions" {...props}>
              <MoreIcon />
            </button>
          )}
        />
      </div>
    </article>
  );
}
