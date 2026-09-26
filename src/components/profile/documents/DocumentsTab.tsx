import { useState } from 'react';

import { useAction } from '../../../hooks/useAction';
import { useOpenLink } from '../../../hooks/useOpenLink';
import { credentialKindLabel, formatPartialDate, isPast } from '../../../lib/documents';
import {
  addProfileDocuments,
  deleteCredential,
  updateProfileDocument,
  type AddDocumentsResult,
  type ProfileCredential,
  type ProfileDocument,
  type ProfileView,
} from '../../../services/profileService';
import { AwardIcon, ExternalIcon, FileIcon, PlusIcon, TrashIcon, UploadIcon } from '../../icons';
import { EmptyState } from '../../ui/EmptyState';
import { IconButton } from '../../ui/IconButton';
import { Section } from '../ProfileSections';
import { CredentialDialog } from './CredentialDialog';
import { DocumentCard } from './DocumentCard';
import { DocumentViewer } from './DocumentViewer';

/**
 * Documents & Credentials, the default part of the Profile: uploaded CVs
 * and optional credentials. Uploading keeps the user here; nothing is
 * copied into the Custom Profile.
 */
export function DocumentsTab({ view }: { view: ProfileView }) {
  const [preview, setPreview] = useState<ProfileDocument | null>(null);
  const [editing, setEditing] = useState<ProfileCredential | 'new' | null>(null);

  // The primary CV first, then the newest.
  const cvs = view.documents
    .filter((d) => d.kind === 'cv')
    .sort((a, b) => Number(b.isPrimary) - Number(a.isPrimary) || b.createdAt - a.createdAt);
  const attached = new Set(view.credentials.flatMap((c) => (c.document ? [c.document.id] : [])));
  const others = view.documents.filter(
    (d) => d.kind !== 'cv' && !attached.has(d.id) && !(d.kind === 'certificate' && editing !== null),
  );

  return (
    <div className="profile">
      <CvSection cvs={cvs} onPreview={setPreview} />
      <CredentialsSection
        credentials={view.credentials}
        onAdd={() => setEditing('new')}
        onEdit={setEditing}
        onPreview={setPreview}
      />
      {others.length > 0 && (
        <Section
          id="profile-other-documents"
          title="Other documents"
          hint="Files added elsewhere, for example to a custom field. Chat mentions them by name when Profile is on."
        >
          <div className="doc-list">
            {others.map((doc) => (
              <DocumentCard
                key={doc.id}
                document={doc}
                onPreview={setPreview}
                extraItems={[
                  { label: 'Use as CV', onSelect: () => void updateProfileDocument(doc.id, doc.name, 'cv').catch(() => {}) },
                ]}
              />
            ))}
          </div>
        </Section>
      )}

      {editing !== null && (
        <CredentialDialog
          credential={editing === 'new' ? null : editing}
          onClose={() => setEditing(null)}
          onPreview={setPreview}
        />
      )}
      {preview && <DocumentViewer document={preview} onClose={() => setPreview(null)} />}
    </div>
  );
}

function UploadSummary({ result, onDismiss }: { result: AddDocumentsResult; onDismiss: () => void }) {
  const added = result.added.length;
  return (
    <div className="upload-summary" role="status">
      {added > 0 && (
        <p className="notice">
          Added {added === 1 ? `“${result.added[0]?.name}”` : `${added} CVs`}.
        </p>
      )}
      {result.failed.length > 0 && (
        <div className="notice notice--danger upload-summary__failed">
          {result.failed.map((f) => (
            <p key={f.name}>
              <strong>{f.name}</strong>: {f.reason}
            </p>
          ))}
        </div>
      )}
      <button type="button" className="link-button upload-summary__dismiss" onClick={onDismiss}>
        Dismiss
      </button>
    </div>
  );
}

function CvSection({ cvs, onPreview }: { cvs: ProfileDocument[]; onPreview: (d: ProfileDocument) => void }) {
  const action = useAction();
  const [result, setResult] = useState<AddDocumentsResult | null>(null);

  const upload = () =>
    void action.run(async () => {
      const outcome = await addProfileDocuments('cv');
      if (outcome.added.length > 0 || outcome.failed.length > 0) setResult(outcome);
    });

  return (
    <Section
      id="profile-cvs"
      title="CVs"
      hint="Your CV files, kept on this computer. When Profile is on in Chat, their text is included, primary CV first."
    >
      {result && <UploadSummary result={result} onDismiss={() => setResult(null)} />}
      {cvs.length === 0 ? (
        <EmptyState
          compact
          icon={<UploadIcon />}
          title="No CVs yet"
          actions={
            <button type="button" className="button button--primary" disabled={action.busy} onClick={upload}>
              <UploadIcon className="button__icon" />
              {action.busy ? 'Adding…' : 'Upload CV'}
            </button>
          }
        >
          PDF or Word (.docx), also text or Markdown. You can upload several; they stay exactly as they are.
        </EmptyState>
      ) : (
        <>
          <div className="doc-list">
            {cvs.map((doc) => (
              <DocumentCard key={doc.id} document={doc} onPreview={onPreview} canBePrimary />
            ))}
          </div>
          <div className="form__inline">
            <button type="button" className="button button--ghost entries__add" disabled={action.busy} onClick={upload}>
              <UploadIcon className="button__icon" />
              {action.busy ? 'Adding…' : 'Upload CVs'}
            </button>
            <span className="form__hint">PDF, Word (.docx), text or Markdown · up to 20 MB each</span>
          </div>
        </>
      )}
      {action.error && <p className="form-error">{action.error}</p>}
    </Section>
  );
}

function CredentialsSection({
  credentials,
  onAdd,
  onEdit,
  onPreview,
}: {
  credentials: ProfileCredential[];
  onAdd: () => void;
  onEdit: (credential: ProfileCredential) => void;
  onPreview: (document: ProfileDocument) => void;
}) {
  return (
    <Section
      id="profile-credentials"
      title="Credentials"
      hint="Optional: degrees, certificates, courses, licenses and badges. Included in Chat when Profile is on."
    >
      {credentials.length === 0 ? (
        <EmptyState
          compact
          icon={<AwardIcon />}
          title="No credentials yet"
          actions={
            <button type="button" className="button button--secondary" onClick={onAdd}>
              <PlusIcon className="button__icon" />
              Add credential
            </button>
          }
        >
          Add a file, details, or both. Everything is optional.
        </EmptyState>
      ) : (
        <>
          <ul className="credential-list">
            {credentials.map((c) => (
              <CredentialRow key={c.id} credential={c} onEdit={() => onEdit(c)} onPreview={onPreview} />
            ))}
          </ul>
          <button type="button" className="button button--ghost entries__add" onClick={onAdd}>
            <PlusIcon className="button__icon" />
            Add credential
          </button>
        </>
      )}
    </Section>
  );
}

function CredentialRow({
  credential: c,
  onEdit,
  onPreview,
}: {
  credential: ProfileCredential;
  onEdit: () => void;
  onPreview: (document: ProfileDocument) => void;
}) {
  const action = useAction();
  const openLink = useOpenLink();
  const [confirming, setConfirming] = useState(false);
  const expired = c.expirationDate !== '' && isPast(c.expirationDate);
  const dates = [
    c.issueDate && `Issued ${formatPartialDate(c.issueDate)}`,
    c.expirationDate && `${expired ? 'Expired' : 'Expires'} ${formatPartialDate(c.expirationDate)}`,
  ].filter(Boolean);

  return (
    <li className="credential">
      <span className="credential__icon" aria-hidden="true">
        <AwardIcon />
      </span>
      <div className="credential__main">
        <div className="credential__title-row">
          <span className="credential__title">{c.title}</span>
          <span className="badge">{credentialKindLabel(c.kind)}</span>
          {expired && <span className="badge badge--warning">Expired</span>}
        </div>
        <span className="credential__meta">{[c.issuer, ...dates].filter(Boolean).join(' · ') || 'No details yet'}</span>
        {(c.credentialId || c.credentialUrl || c.document) && (
          <span className="credential__links">
            {c.credentialId && <span>ID {c.credentialId}</span>}
            {c.credentialUrl && (
              <button type="button" className="link-button" onClick={(e) => openLink(c.credentialUrl, e)}>
                <ExternalIcon className="button__icon" />
                Verify
              </button>
            )}
            {c.document && (
              <button type="button" className="link-button" onClick={() => c.document && onPreview(c.document)}>
                <FileIcon className="button__icon" />
                {c.document.name}
              </button>
            )}
          </span>
        )}
        {c.note && <span className="credential__note">{c.note}</span>}
        {confirming && (
          <div className="doc-card__confirm" role="group" aria-label="Confirm removal">
            <span>Remove this credential{c.document ? ' and its file' : ''}?</span>
            <button
              type="button"
              className="button button--danger button--small"
              disabled={action.busy}
              onClick={() => void action.run(() => deleteCredential(c.id))}
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
      <div className="credential__actions">
        <button type="button" className="button button--ghost button--small" onClick={onEdit}>
          Edit
        </button>
        <IconButton label="Remove credential" className="icon-button--small" onClick={() => setConfirming(true)}>
          <TrashIcon />
        </IconButton>
      </div>
    </li>
  );
}
