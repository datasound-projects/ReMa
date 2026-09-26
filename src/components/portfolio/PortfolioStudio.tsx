import { useState } from 'react';

import { dataOr } from '../../hooks/useAsyncData';
import { usePortfolios } from '../../hooks/usePortfolios';
import { useAction } from '../../hooks/useAction';
import { formatDateTime } from '../../lib/format';
import { PAGE_SIZES } from '../../lib/portfolio/layout';
import { DEFAULT_TEMPLATE_ID, TEMPLATES, templateById } from '../../lib/portfolio/templates';
import {
  deletePortfolio,
  duplicatePortfolio,
  type PortfolioDocument,
} from '../../services/portfolioService';
import { LayoutIcon, MoreIcon, PlusIcon } from '../icons';
import { EmptyState, LoadingState } from '../ui/EmptyState';
import { Menu } from '../ui/Menu';
import { NewPortfolioDialog } from './NewPortfolioDialog';
import { PortfolioEditor } from './PortfolioEditor';
import { TemplateThumbnail } from './TemplateThumbnail';

interface PortfolioStudioProps {
  /** The document open in the editor (from the navigation state). */
  openId: number | null;
  onOpen: (id: number | null) => void;
  profileAvailable: boolean;
}

/**
 * CVs built in ReMa. Separate from uploaded files, which are never
 * changed: a CV here is its own document with its own design.
 */
export function PortfolioStudio({ openId, onOpen, profileAvailable }: PortfolioStudioProps) {
  const loaded = usePortfolios();
  const documents = dataOr(loaded.state, [] as PortfolioDocument[]);
  const [creating, setCreating] = useState<string | null>(null);

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
          title="This CV no longer exists"
          actions={
            <button type="button" className="button button--secondary" onClick={() => onOpen(null)}>
              All CVs
            </button>
          }
        >
          It may have been deleted.
        </EmptyState>
      );
    }
    return <PortfolioEditor key={open.id} document={open} onClose={() => onOpen(null)} onOpen={onOpen} />;
  }

  return (
    <div className="studio">
      <section className="section" aria-labelledby="studio-mine">
        <div className="section__head">
          <div className="section__heading">
            <h2 className="section__title" id="studio-mine">
              Your CVs
            </h2>
            <p className="section__description">
              Build polished CVs from scratch and export them as PDF. Uploaded files are never changed.
            </p>
          </div>
          <div className="section__actions">
            <button type="button" className="button button--primary" onClick={() => setCreating(DEFAULT_TEMPLATE_ID)}>
              <PlusIcon className="button__icon" />
              New CV
            </button>
          </div>
        </div>
        {documents.length === 0 ? (
          <EmptyState
            framed
            icon={<LayoutIcon />}
            title="No CVs yet"
            actions={
              <button type="button" className="button button--secondary" onClick={() => setCreating(DEFAULT_TEMPLATE_ID)}>
                <PlusIcon className="button__icon" />
                New CV
              </button>
            }
          >
            Pick a template below, or start with the default one.
          </EmptyState>
        ) : (
          <div className="studio-docs">
            {documents.map((doc) => (
              <PortfolioCard key={doc.id} document={doc} onOpen={() => onOpen(doc.id)} onOpenCopy={onOpen} />
            ))}
          </div>
        )}
      </section>

      <section className="section" aria-labelledby="studio-templates">
        <div className="section__head">
          <div className="section__heading">
            <h2 className="section__title" id="studio-templates">
              Templates
            </h2>
            <p className="section__description">
              {TEMPLATES.length} designs. Switch any time in the editor; your content stays.
            </p>
          </div>
        </div>
        <div className="template-gallery">
          {TEMPLATES.map((t) => (
            <button key={t.id} type="button" className="template-card" onClick={() => setCreating(t.id)}>
              <TemplateThumbnail templateId={t.id} name={t.name} />
              <span className="template-card__text">
                <span className="template-card__name">{t.name}</span>
                <span className="template-card__description">{t.description}</span>
              </span>
            </button>
          ))}
        </div>
      </section>

      {creating !== null && (
        <NewPortfolioDialog
          initialTemplate={creating}
          profileAvailable={profileAvailable}
          onClose={() => setCreating(null)}
          onCreated={(doc) => {
            setCreating(null);
            onOpen(doc.id);
          }}
        />
      )}
    </div>
  );
}

function PortfolioCard({
  document: doc,
  onOpen,
  onOpenCopy,
}: {
  document: PortfolioDocument;
  onOpen: () => void;
  onOpenCopy: (id: number) => void;
}) {
  const action = useAction();
  const [confirming, setConfirming] = useState(false);
  const template = templateById(doc.templateId);
  return (
    <article className="studio-doc" aria-label={doc.name}>
      <button type="button" className="studio-doc__open" onClick={onOpen}>
        <TemplateThumbnail templateId={doc.templateId} pageSize={doc.pageSize} name={template.name} />
      </button>
      <div className="studio-doc__body">
        <button type="button" className="studio-doc__name" onClick={onOpen}>
          {doc.name}
        </button>
        <span className="studio-doc__meta">
          {template.name} · {PAGE_SIZES[doc.pageSize].label} · edited {formatDateTime(doc.updatedAt)}
        </span>
        {confirming ? (
          <div className="doc-card__confirm" role="group" aria-label="Confirm deletion">
            <span>Delete this CV?</span>
            <button
              type="button"
              className="button button--danger button--small"
              disabled={action.busy}
              onClick={() => void action.run(() => deletePortfolio(doc.id))}
            >
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
            <Menu
              items={[
                {
                  label: 'Duplicate',
                  onSelect: () =>
                    void action.run(async () => {
                      const copy = await duplicatePortfolio(doc.id);
                      onOpenCopy(copy.id);
                    }),
                },
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
