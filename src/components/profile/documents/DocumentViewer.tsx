import { useEffect, useRef, useState } from 'react';

import { useCoverBrowser } from '../../../app/browser';
import { formatDateTime } from '../../../lib/format';
import { formatSize, FORMAT_LABELS } from '../../../lib/documents';
import { toApiError } from '../../../services/ipc';
import {
  documentUrl,
  openProfileDocument,
  profileDocumentBlocks,
  type DocumentBlock,
  type ProfileDocument,
} from '../../../services/profileService';
import { CloseIcon, ExternalIcon, ZoomInIcon, ZoomOutIcon } from '../../icons';
import { LazyPdfPages } from '../../pdf/LazyPdfPages';
import { LoadingState } from '../../ui/EmptyState';
import { IconButton } from '../../ui/IconButton';

const ZOOMS = [0.5, 0.75, 1, 1.25, 1.5, 2];

/**
 * Shows a stored document inside ReMa, safely: PDFs are drawn by PDF.js,
 * images as images, and Word/text files as plain text blocks (never as
 * HTML). Closing it leaves the Profile exactly as it was.
 */
export function DocumentViewer({ document: doc, onClose }: { document: ProfileDocument; onClose: () => void }) {
  const ref = useRef<HTMLDialogElement>(null);
  const bodyRef = useRef<HTMLDivElement>(null);
  useCoverBrowser();
  useEffect(() => {
    const dialog = ref.current;
    if (dialog && !dialog.open) dialog.showModal();
    // The document gets focus (arrow keys scroll it), not the first button.
    bodyRef.current?.focus();
  }, []);

  const isPdf = doc.format === 'pdf';
  const isImage = doc.format === 'png' || doc.format === 'jpeg' || doc.format === 'webp';
  const [zoom, setZoom] = useState(1);
  const [openError, setOpenError] = useState<string | null>(null);

  return (
    <dialog
      ref={ref}
      className="dialog viewer"
      aria-label={`Preview of ${doc.name}`}
      onCancel={(event) => {
        event.preventDefault();
        onClose();
      }}
      onMouseDown={(event) => {
        if (event.target === ref.current) onClose();
      }}
    >
      <header className="viewer__header">
        <div className="viewer__title">
          <h2 className="viewer__name">{doc.name}</h2>
          <span className="viewer__meta">
            {FORMAT_LABELS[doc.format]} · {formatSize(doc.size)} · added {formatDateTime(doc.createdAt)}
          </span>
        </div>
        {(isPdf || isImage) && (
          <div className="viewer__zoom" role="group" aria-label="Zoom">
            <IconButton
              label="Zoom out"
              className="icon-button--small"
              disabled={zoom <= (ZOOMS[0] ?? 1)}
              onClick={() => setZoom((z) => ZOOMS.filter((v) => v < z).at(-1) ?? z)}
            >
              <ZoomOutIcon />
            </IconButton>
            <button type="button" className="viewer__zoom-value" title="Fit to width" onClick={() => setZoom(1)}>
              {Math.round(zoom * 100)}%
            </button>
            <IconButton
              label="Zoom in"
              className="icon-button--small"
              disabled={zoom >= (ZOOMS.at(-1) ?? 1)}
              onClick={() => setZoom((z) => ZOOMS.find((v) => v > z) ?? z)}
            >
              <ZoomInIcon />
            </IconButton>
          </div>
        )}
        <button
          type="button"
          className="button button--ghost"
          title="Open with the default app on this computer"
          onClick={() => {
            setOpenError(null);
            openProfileDocument(doc.id).catch((err: unknown) => setOpenError(toApiError(err).message));
          }}
        >
          <ExternalIcon className="button__icon" />
          Open in app
        </button>
        <IconButton label="Close preview" onClick={onClose}>
          <CloseIcon />
        </IconButton>
      </header>
      {openError && (
        <p className="form-error viewer__error" role="alert">
          {openError}
        </p>
      )}
      <div className="viewer__body" ref={bodyRef} tabIndex={-1}>
        {isPdf ? (
          <PdfView document={doc} zoom={zoom} />
        ) : isImage ? (
          <ImageView document={doc} zoom={zoom} />
        ) : (
          <TextView document={doc} />
        )}
      </div>
    </dialog>
  );
}

function Fallback({ message }: { message: string }) {
  return (
    <div className="viewer__fallback" role="status">
      <p>{message}</p>
      <p className="form__hint">You can still open it with the default app on this computer.</p>
    </div>
  );
}

function PdfView({ document: doc, zoom }: { document: ProfileDocument; zoom: number }) {
  const [data, setData] = useState<Uint8Array | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [rendered, setRendered] = useState(false);

  useEffect(() => {
    let active = true;
    fetch(documentUrl(doc.id), { cache: 'no-store' })
      .then(async (response) => {
        if (!response.ok) throw new Error(await response.text());
        return new Uint8Array(await response.arrayBuffer());
      })
      .then((bytes) => active && setData(bytes))
      .catch(() => active && setError('The file could not be read.'));
    return () => {
      active = false;
    };
  }, [doc.id, doc.updatedAt]);

  if (error) return <Fallback message={error} />;
  return (
    <div className="viewer__pdf">
      {!rendered && <LoadingState label="Opening the PDF…" />}
      <LazyPdfPages
        data={data}
        zoom={zoom}
        label={`Pages of ${doc.name}`}
        onRendered={() => setRendered(true)}
        onError={setError}
      />
    </div>
  );
}

function ImageView({ document: doc, zoom }: { document: ProfileDocument; zoom: number }) {
  const [failed, setFailed] = useState(false);
  if (failed) return <Fallback message="This image could not be shown." />;
  return (
    <div className="viewer__image">
      <img
        src={`${documentUrl(doc.id)}?v=${doc.updatedAt}`}
        alt={doc.name}
        style={{ width: `${zoom * 100}%` }}
        draggable={false}
        onError={() => setFailed(true)}
      />
    </div>
  );
}

function TextView({ document: doc }: { document: ProfileDocument }) {
  const [blocks, setBlocks] = useState<DocumentBlock[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let active = true;
    profileDocumentBlocks(doc.id)
      .then((b) => active && setBlocks(b))
      .catch((err: unknown) => active && setError(toApiError(err).message));
    return () => {
      active = false;
    };
  }, [doc.id, doc.updatedAt]);

  if (error) return <Fallback message={error} />;
  if (!blocks) return <LoadingState label="Reading the document…" />;
  if (blocks.length === 0) return <Fallback message="This document has no readable text." />;
  return (
    <article className="viewer__text">
      <p className="viewer__note">
        {doc.format === 'docx'
          ? 'Text of the Word document. Layout, images and any macros are left out.'
          : 'Plain text of the document.'}
      </p>
      {blocks.map((block, index) => (
        <Block key={index} block={block} />
      ))}
    </article>
  );
}

function Block({ block }: { block: DocumentBlock }) {
  switch (block.type) {
    case 'heading': {
      const level = Math.min(Math.max(block.level, 1), 4);
      const Tag = (['h3', 'h3', 'h4', 'h5'] as const)[level - 1] ?? 'h5';
      return <Tag className={`viewer__h viewer__h--${level}`}>{block.text}</Tag>;
    }
    case 'paragraph':
      return <p className="viewer__p">{block.text}</p>;
    case 'listItem':
      return (
        <p className="viewer__li" style={{ marginInlineStart: `${1 + Math.min(block.level, 6) * 1.25}em` }}>
          {block.text}
        </p>
      );
    case 'table':
      return (
        <div className="table-wrap viewer__table">
          <table>
            <tbody>
              {block.rows.map((row, r) => (
                <tr key={r}>
                  {row.map((cell, c) => (
                    <td key={c}>{cell}</td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      );
  }
}
