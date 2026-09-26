import { useRef, useState } from 'react';

import { CREDENTIAL_KINDS, FORMAT_LABELS, formatSize } from '../../../lib/documents';
import { toApiError } from '../../../services/ipc';
import {
  addProfileDocument,
  deleteProfileDocument,
  saveCredential,
  type CredentialInput,
  type CredentialKind,
  type ProfileCredential,
  type ProfileDocument,
} from '../../../services/profileService';
import { FileIcon, UploadIcon } from '../../icons';
import { Dialog } from '../../ui/Dialog';

interface CredentialDialogProps {
  /** `null` adds a new credential. */
  credential: ProfileCredential | null;
  onClose: () => void;
  onPreview: (document: ProfileDocument) => void;
}

const empty = (): CredentialInput => ({
  kind: 'professional_certificate',
  title: '',
  issuer: '',
  issueDate: '',
  expirationDate: '',
  credentialId: '',
  credentialUrl: '',
  note: '',
});

const fromCredential = (c: ProfileCredential): CredentialInput => ({
  kind: c.kind,
  title: c.title,
  issuer: c.issuer,
  issueDate: c.issueDate,
  expirationDate: c.expirationDate,
  credentialId: c.credentialId,
  credentialUrl: c.credentialUrl,
  note: c.note,
});

/**
 * Adds or edits a credential. Every detail is optional; a file can be
 * attached (PDF, image or Word). A file attached here and then cancelled
 * is removed again, so nothing is left behind.
 */
export function CredentialDialog({ credential, onClose, onPreview }: CredentialDialogProps) {
  const [form, setForm] = useState<CredentialInput>(() => (credential ? fromCredential(credential) : empty()));
  const [file, setFile] = useState<ProfileDocument | null>(credential?.document ?? null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Files picked in this dialog that are not saved yet.
  const unsaved = useRef(new Set<number>());

  const set = (patch: Partial<CredentialInput>) => setForm((f) => ({ ...f, ...patch }));

  const discardUnsaved = (keep: number | null) => {
    for (const id of unsaved.current) {
      if (id !== keep) void deleteProfileDocument(id).catch(() => {});
    }
    unsaved.current.clear();
  };

  const attach = async () => {
    setError(null);
    try {
      const doc = await addProfileDocument('certificate');
      if (!doc) return;
      unsaved.current.add(doc.id);
      // Replacing a file picked in this dialog: the earlier pick goes.
      if (file && unsaved.current.has(file.id) && file.id !== doc.id) {
        unsaved.current.delete(file.id);
        void deleteProfileDocument(file.id).catch(() => {});
      }
      setFile(doc);
    } catch (err) {
      setError(toApiError(err).message);
    }
  };

  const detach = () => {
    if (file && unsaved.current.has(file.id)) {
      unsaved.current.delete(file.id);
      void deleteProfileDocument(file.id).catch(() => {});
    }
    setFile(null);
  };

  const cancel = () => {
    discardUnsaved(null);
    onClose();
  };

  const save = async () => {
    setBusy(true);
    setError(null);
    try {
      await saveCredential(credential?.id ?? null, form, file?.id ?? null);
      // The saved file is now the credential's; the old one (if replaced)
      // was removed by ReMa.
      if (file) unsaved.current.delete(file.id);
      discardUnsaved(null);
      onClose();
    } catch (err) {
      setError(toApiError(err).message);
    } finally {
      setBusy(false);
    }
  };

  const canSave = !busy && (form.title.trim() !== '' || file !== null);

  return (
    <Dialog
      title={credential ? 'Edit credential' : 'Add credential'}
      onClose={cancel}
      size="wide"
      actions={
        <>
          {error && <span className="form-error">{error}</span>}
          <button type="button" className="button button--secondary" onClick={cancel}>
            Cancel
          </button>
          <button type="button" className="button button--primary" disabled={!canSave} onClick={() => void save()}>
            {busy ? 'Saving…' : credential ? 'Save' : 'Add credential'}
          </button>
        </>
      }
    >
      <p className="dialog__text">
        All details are optional. Stored on this computer; Chat uses them only when you turn Profile on.
      </p>
      <div className="credential-form">
        <label className="field">
          <span className="field__label">Type</span>
          <select
            className="input"
            value={form.kind ?? 'other'}
            onChange={(e) => set({ kind: e.target.value as CredentialKind })}
          >
            {CREDENTIAL_KINDS.map((k) => (
              <option key={k.id} value={k.id}>
                {k.label}
              </option>
            ))}
          </select>
        </label>
        <label className="field">
          <span className="field__label">Title</span>
          <input
            className="input"
            placeholder="e.g. AWS Certified Solutions Architect"
            maxLength={200}
            value={form.title}
            onChange={(e) => set({ title: e.target.value })}
          />
        </label>
        <label className="field">
          <span className="field__label">Issuer</span>
          <input
            className="input"
            placeholder="e.g. Amazon Web Services"
            maxLength={200}
            value={form.issuer}
            onChange={(e) => set({ issuer: e.target.value })}
          />
        </label>
        <label className="field">
          <span className="field__label">Credential ID</span>
          <input
            className="input"
            maxLength={200}
            value={form.credentialId}
            onChange={(e) => set({ credentialId: e.target.value })}
          />
        </label>
        <label className="field">
          <span className="field__label">Issued</span>
          <input
            className="input"
            placeholder="YYYY-MM"
            inputMode="numeric"
            maxLength={10}
            value={form.issueDate}
            onChange={(e) => set({ issueDate: e.target.value })}
          />
        </label>
        <label className="field">
          <span className="field__label">Expires</span>
          <input
            className="input"
            placeholder="YYYY-MM (optional)"
            inputMode="numeric"
            maxLength={10}
            value={form.expirationDate}
            onChange={(e) => set({ expirationDate: e.target.value })}
          />
        </label>
        <label className="field credential-form__wide">
          <span className="field__label">Credential URL</span>
          <input
            className="input"
            type="url"
            placeholder="https://"
            maxLength={2000}
            value={form.credentialUrl}
            onChange={(e) => set({ credentialUrl: e.target.value })}
          />
        </label>
        <label className="field credential-form__wide">
          <span className="field__label">Note</span>
          <textarea
            className="input input--textarea"
            rows={2}
            maxLength={2000}
            value={form.note}
            onChange={(e) => set({ note: e.target.value })}
          />
        </label>
      </div>
      <div className="field">
        <span className="field__label">File</span>
        {file ? (
          <div className="attached-file">
            <FileIcon className="attached-file__icon" />
            <button type="button" className="attached-file__name" onClick={() => onPreview(file)}>
              {file.name}
            </button>
            <span className="attached-file__meta">
              {FORMAT_LABELS[file.format]} · {formatSize(file.size)}
            </span>
            <button type="button" className="button button--ghost button--small" onClick={() => void attach()}>
              Replace
            </button>
            <button type="button" className="button button--ghost button--small" onClick={detach}>
              Remove
            </button>
          </div>
        ) : (
          <div className="form__inline">
            <button type="button" className="button button--secondary" onClick={() => void attach()}>
              <UploadIcon className="button__icon" />
              Attach file
            </button>
            <span className="form__hint">PDF, PNG, JPEG, WebP or Word (.docx) · up to 20 MB</span>
          </div>
        )}
      </div>
    </Dialog>
  );
}
