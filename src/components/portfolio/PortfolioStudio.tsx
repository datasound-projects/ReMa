import { useMemo, useState } from 'react';

import { dataOr } from '../../hooks/useAsyncData';
import { useAction } from '../../hooks/useAction';
import { usePortfolios } from '../../hooks/usePortfolios';
import { formatDateTime } from '../../lib/format';
import { PAGE_SIZES } from '../../lib/portfolio/layout';
import { templateById, type Template } from '../../lib/portfolio/templates';
import { documentInput } from '../../lib/portfolio/thumbnails';
import {
  createPortfolio,
  deletePortfolio,
  duplicatePortfolio,
  renamePortfolio,
  type PortfolioDocument,
  type PortfolioKind,
  type PortfolioStart,
} from '../../services/portfolioService';
import { FileIcon, LayoutIcon, MailIcon, MoreIcon, PlusIcon, ProfileIcon, UploadIcon } from '../icons';
import { EmptyState, LoadingState } from '../ui/EmptyState';
import { Menu } from '../ui/Menu';
import { ImportDialog } from './ImportDialog';
import { NewPortfolioDialog } from './NewPortfolioDialog';
import { PortfolioEditor } from './PortfolioEditor';
import { TemplateGallery } from './TemplateGallery';
import { TemplatePreviewDialog } from './TemplatePreviewDialog';
import { DocumentThumbnail } from './TemplateThumbnail';

interface PortfolioStudioProps {
  /** The document open in the editor (from the navigation state). */
  openId: number | null;
  onOpen: (id: number | null) => void;
  profileAvailable: boolean;
}

type Tab = 'documents' | 'cv' | 'letters';
type Creating = { kind: PortfolioKind; templateId?: string; start?: PortfolioStart } | null;

/**
 * CVs and cover letters built in ReMa. Separate from uploaded files, which
 * are never changed: a document here has its own content and design.
 */
export function PortfolioStudio({ openId, onOpen, profileAvailable }: PortfolioStudioProps) {
  const loaded = usePortfolios();
  const documents = dataOr(loaded.state, [] as PortfolioDocument[]);
  const [tab, setTab] = useState<Tab>('documents');
  const [creating, setCreating] = useState<Creating>(null);
  const [importing, setImporting] = useState(false);
  const [preview, setPreview] = useState<{ template: Template; documentId?: number } | null>(null);

  if (loaded.state.status === 'loading') return <LoadingState label="Loading Portfolio Studio…" />;
  if (loaded.state.status === 'error') {
    return (
      <div className="notice notice--danger" role="alert">
        {loaded.state.error.message}{' '}
        <button type="button" className="link-button" onClick={loaded.retry}>
          Try again
        </button>
      </div>
    );
  }

  if (openId !== null) {
    const open = documents.find((d) => d.id === openId);
    if (!open) {
      return (
        <EmptyState
          framed
          icon={<LayoutIcon />}
          title="This document no longer exists"
          actions={
            <button type="button" className="button button--secondary" onClick={() => onOpen(null)}>
              All documents
            </button>
          }
        >
          It may have been deleted.
        </EmptyState>
      );
    }
    return <PortfolioEditor key={open.id} document={open} profileAvailable={profileAvailable} onClose={() => onOpen(null)} onOpen={onOpen} />;
  }

  const tabs: { id: Tab; label: string; count?: number }[] = [
    { id: 'documents', label: 'My Documents', count: documents.length },
    { id: 'cv', label: 'CV Templates' },
    { id: 'letters', label: 'Cover Letter Templates' },
  ];

  return (
    <div className="studio">
      <div className="studio-head">
        <div className="profile-tabs studio-tabs" role="tablist" aria-label="Portfolio Studio">
          {tabs.map((t) => (
            <button
              key={t.id}
              type="button"
              role="tab"
              aria-selected={tab === t.id}
              className={tab === t.id ? 'profile-tabs__tab profile-tabs__tab--active' : 'profile-tabs__tab'}
              onClick={() => setTab(t.id)}
            >
              {t.label}
              {t.count !== undefined && t.count > 0 && <span className="studio-tabs__count">{t.count}</span>}
            </button>
          ))}
        </div>
        <div className="studio-actions">
          <button type="button" className="button button--primary" onClick={() => setCreating({ kind: 'cv' })}>
            <PlusIcon className="button__icon" />
            Create CV
          </button>
          <button type="button" className="button button--secondary" onClick={() => setCreating({ kind: 'cover_letter' })}>
            <MailIcon className="button__icon" />
            Create Cover Letter
          </button>
          <button type="button" className="button button--secondary" onClick={() => setImporting(true)}>
            <UploadIcon className="button__icon" />
            Import Existing Document
          </button>
          <button
            type="button"
            className="button button--secondary"
            title={profileAvailable ? 'A CV filled from your Custom Profile' : 'Fill in your Custom Profile first'}
            disabled={!profileAvailable}
            onClick={() => setCreating({ kind: 'cv', start: 'custom_profile' })}
          >
            <ProfileIcon className="button__icon" />
            Start from Profile
          </button>
        </div>
      </div>

      {tab === 'documents' && (
        <section aria-label="My Documents">
          {documents.length === 0 ? (
            <EmptyState
              framed
              icon={<LayoutIcon />}
              title="No documents yet"
              actions={
                <>
                  <button type="button" className="button button--primary" onClick={() => setCreating({ kind: 'cv' })}>
                    <PlusIcon className="button__icon" />
                    Create CV
                  </button>
                  <button type="button" className="button button--secondary" onClick={() => setTab('cv')}>
                    Browse templates
                  </button>
                </>
              }
            >
              Create a CV or cover letter, import an existing file, or start from your Profile.
            </EmptyState>
          ) : (
            <div className="studio-docs">
              {documents.map((doc) => (
                <PortfolioCard
                  key={doc.id}
                  document={doc}
                  onOpen={() => onOpen(doc.id)}
                  onOpenCopy={onOpen}
                  onPreview={() => setPreview({ template: templateById(doc.templateId, doc.kind === 'cover_letter' ? 'letter' : 'cv'), documentId: doc.id })}
                />
              ))}
            </div>
          )}
        </section>
      )}

      {tab === 'cv' && (
        <section aria-label="CV Templates">
          <p className="studio-intro">
            Twenty designs in ten categories. Any template works for any profession, and switching later keeps your content.
          </p>
          <TemplateGallery
            kind="cv"
            actionLabel="Use"
            onSelect={(t) => setCreating({ kind: 'cv', templateId: t.id })}
            onPreview={(template) => setPreview({ template })}
          />
        </section>
      )}

      {tab === 'letters' && (
        <section aria-label="Cover Letter Templates">
          <p className="studio-intro">Five letter designs that match the CV templates’ fonts and colors.</p>
          <TemplateGallery
            kind="letter"
            actionLabel="Use"
            onSelect={(t) => setCreating({ kind: 'cover_letter', templateId: t.id })}
            onPreview={(template) => setPreview({ template })}
          />
        </section>
      )}

      {creating && (
        <NewPortfolioDialog
          kind={creating.kind}
          initialTemplate={creating.templateId}
          initialStart={creating.start}
          profileAvailable={profileAvailable}
          onClose={() => setCreating(null)}
          onCreated={(doc) => {
            setCreating(null);
            onOpen(doc.id);
          }}
        />
      )}
      {importing && (
        <ImportDialog
          onClose={() => setImporting(false)}
          onCreated={(doc) => {
            setImporting(false);
            onOpen(doc.id);
          }}
        />
      )}
      {preview && (
        <TemplatePreviewDialog
          templateId={preview.template.id}
          current={preview.documentId !== undefined ? documentInput(documents.find((d) => d.id === preview.documentId) ?? documents[0]!) : null}
          actionLabel={preview.documentId !== undefined ? 'Open in editor' : 'Use this template'}
          onAction={(templateId) => {
            const docId = preview.documentId;
            setPreview(null);
            if (docId !== undefined) onOpen(docId);
            else setCreating({ kind: preview.template.kind === 'letter' ? 'cover_letter' : 'cv', templateId });
          }}
          onClose={() => setPreview(null)}
        />
      )}
    </div>
  );
}

function PortfolioCard({
  document: doc,
  onOpen,
  onOpenCopy,
  onPreview,
}: {
  document: PortfolioDocument;
  onOpen: () => void;
  onOpenCopy: (id: number) => void;
  onPreview: () => void;
}) {
  const action = useAction();
  const [confirming, setConfirming] = useState(false);
  const [renaming, setRenaming] = useState<string | null>(null);
  const kind = doc.kind ?? 'cv';
  const template = templateById(doc.templateId, kind === 'cover_letter' ? 'letter' : 'cv');
  const input = useMemo(() => documentInput(doc), [doc]);

  const rename = async () => {
    const name = (renaming ?? '').trim();
    if (!name || name === doc.name) {
      setRenaming(null);
      return;
    }
    if (await action.run(() => renamePortfolio(doc.id, name))) setRenaming(null);
  };

  return (
    <article className="studio-doc" aria-label={doc.name}>
      <button type="button" className="studio-doc__open" onClick={onOpen} aria-label={`Edit ${doc.name}`}>
        <DocumentThumbnail cacheKey={`${doc.id}:${doc.updatedAt}`} input={input} name={doc.name} />
      </button>
      <div className="studio-doc__body">
        {renaming !== null ? (
          <form
            className="studio-doc__rename"
            onSubmit={(e) => {
              e.preventDefault();
              void rename();
            }}
          >
            <input
              className="input input--small"
              aria-label="Document name"
              maxLength={120}
              value={renaming}
              autoFocus
              onChange={(e) => setRenaming(e.target.value)}
              onKeyDown={(e) => e.key === 'Escape' && setRenaming(null)}
              onBlur={() => void rename()}
            />
          </form>
        ) : (
          <button type="button" className="studio-doc__name" onClick={onOpen} title={doc.name}>
            {doc.name}
          </button>
        )}
        <span className="studio-doc__meta">
          <span className={kind === 'cover_letter' ? 'studio-doc__kind studio-doc__kind--letter' : 'studio-doc__kind'}>
            {kind === 'cover_letter' ? <MailIcon /> : <FileIcon />}
            {kind === 'cover_letter' ? 'Cover letter' : 'CV'}
          </span>
          <span>{template.name}</span>
          <span>{PAGE_SIZES[doc.pageSize].label}</span>
        </span>
        <span className="studio-doc__edited">Edited {formatDateTime(doc.updatedAt)}</span>
        {confirming ? (
          <div className="doc-card__confirm" role="group" aria-label="Confirm deletion">
            <span>Delete this document?</span>
            <button type="button" className="button button--danger button--small" disabled={action.busy} onClick={() => void action.run(() => deletePortfolio(doc.id))}>
              Delete
            </button>
            <button type="button" className="button button--ghost button--small" onClick={() => setConfirming(false)}>
              Cancel
            </button>
          </div>
        ) : (
          <div className="studio-doc__actions">
            <button type="button" className="button button--secondary button--small" onClick={onOpen}>
              Edit
            </button>
            <button type="button" className="button button--ghost button--small" onClick={onPreview}>
              Preview
            </button>
            <Menu
              items={[
                { label: 'Rename', onSelect: () => setRenaming(doc.name) },
                {
                  label: 'Duplicate',
                  onSelect: () =>
                    void action.run(async () => {
                      const copy = await duplicatePortfolio(doc.id);
                      onOpenCopy(copy.id);
                    }),
                },
                ...(kind === 'cv'
                  ? [
                      {
                        label: 'Matching cover letter',
                        onSelect: () =>
                          void action.run(async () => {
                            const { matchingLetterTemplate } = await import('../../lib/portfolio/templates');
                            const letter = await createPortfolio({
                              name: `${doc.name} — cover letter`,
                              kind: 'cover_letter',
                              templateId: matchingLetterTemplate(doc.templateId).id,
                              pageSize: doc.pageSize,
                              content: { header: doc.content.header, sections: [] },
                              letter: { greeting: 'Dear Hiring Manager,', closing: 'Kind regards,', signature: doc.content.header.fullName },
                            });
                            onOpenCopy(letter.id);
                          }),
                      },
                    ]
                  : []),
                { label: 'Delete', danger: true, onSelect: () => setConfirming(true) },
              ]}
              trigger={(props) => (
                <button type="button" className="icon-button icon-button--small" aria-label="More actions" title="More actions" {...props}>
                  <MoreIcon />
                </button>
              )}
            />
          </div>
        )}
        {action.error && <p className="form-error">{action.error}</p>}
      </div>
    </article>
  );
}
