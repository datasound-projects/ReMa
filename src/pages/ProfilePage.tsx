import { useState } from 'react';

import { ProfileIcon, UploadIcon } from '../components/icons';
import { PageContainer } from '../components/layout/PageContainer';
import { LoadingState } from '../components/ui/EmptyState';
import { CustomFieldsSection, DocumentsSection } from '../components/profile/DocumentsSection';
import { ImportReviewDialog } from '../components/profile/ImportReviewDialog';
import { ProfileOverview } from '../components/profile/ProfileOverview';
import {
  EducationSection,
  ExperienceSection,
  LanguagesSection,
  LinksSection,
  PersonalSection,
  ProfessionalSection,
} from '../components/profile/ProfileSections';
import { dataOr } from '../hooks/useAsyncData';
import { useProfile } from '../hooks/useProfile';
import { toApiError } from '../services/ipc';
import {
  addProfileDocument,
  emptyProfile,
  importProfileDocument,
  saveProfile,
  type Profile,
  type ProfileDocument,
  type ProfileImport,
} from '../services/profileService';

const sameProfile = (a: Profile, b: Profile) => JSON.stringify(a) === JSON.stringify(b);

/**
 * The user's reusable career profile: one page, edited as a draft and saved
 * explicitly. Imports from a CV only propose values for review.
 */
export function ProfilePage() {
  const loaded = useProfile();
  const view = dataOr(loaded.state, null);
  const saved = view?.profile ?? null;
  const documents = view?.documents ?? [];

  // The draft follows the saved profile until the user edits it.
  const [draft, setDraft] = useState<Profile | null>(null);
  const current = draft ?? saved ?? emptyProfile();
  const dirty = draft !== null && saved !== null && !sameProfile(draft, saved);
  const update = (patch: Partial<Profile>) => setDraft({ ...current, ...patch });

  const [building, setBuilding] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [importing, setImporting] = useState<number | null>(null);
  const [review, setReview] = useState<ProfileImport | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const save = async (profile: Profile) => {
    setSaving(true);
    setError(null);
    try {
      await saveProfile(profile);
      setDraft(null);
      loaded.refresh();
      return true;
    } catch (err) {
      setDraft(profile);
      setError(toApiError(err).message);
      return false;
    } finally {
      setSaving(false);
    }
  };

  const importDocument = async (doc: ProfileDocument) => {
    setImporting(doc.id);
    setError(null);
    try {
      setReview(await importProfileDocument(doc.id));
    } catch (err) {
      setError(toApiError(err).message);
    } finally {
      setImporting(null);
    }
  };

  const importCv = async () => {
    setError(null);
    try {
      const doc = await addProfileDocument('cv');
      if (doc) await importDocument(doc);
    } catch (err) {
      setError(toApiError(err).message);
    }
  };

  if (loaded.state.status === 'loading') {
    return (
      <PageContainer title="Profile">
        <LoadingState label="Loading your Profile…" />
      </PageContainer>
    );
  }
  if (loaded.state.status === 'error') {
    return (
      <PageContainer title="Profile">
        <p className="form-error">{loaded.state.error.message}</p>
      </PageContainer>
    );
  }

  const isEmpty = sameProfile(current, emptyProfile()) && documents.length === 0;

  return (
    <PageContainer
      title="Profile"
      subtitle="Your career details, entered once. ReMa Auto Fill uses them, and Chat does when you turn Profile on."
      actions={
        !isEmpty && (
          <button
            type="button"
            className="button button--secondary"
            disabled={importing !== null}
            onClick={() => void importCv()}
          >
            <UploadIcon className="button__icon" />
            {importing !== null ? 'Reading…' : 'Import CV'}
          </button>
        )
      }
    >
      {error && !dirty && (
        <p className="form-error profile__error" role="alert">
          {error}
        </p>
      )}
      {notice && (
        <p className="notice" role="status">
          {notice}
        </p>
      )}

      {isEmpty && !building ? (
        <div className="profile-start">
          <button
            type="button"
            className="profile-start__card"
            disabled={importing !== null}
            onClick={() => void importCv()}
          >
            <span className="profile-start__icon" aria-hidden="true">
              <UploadIcon />
            </span>
            <span className="profile-start__title">
              {importing !== null ? 'Reading your CV…' : 'Import your CV'}
            </span>
            <span className="profile-start__text">
              PDF, Word, text or Markdown. ReMa reads it and you review every detail before it is saved.
            </span>
          </button>
          <button type="button" className="profile-start__card" onClick={() => setBuilding(true)}>
            <span className="profile-start__icon" aria-hidden="true">
              <ProfileIcon />
            </span>
            <span className="profile-start__title">Build it yourself</span>
            <span className="profile-start__text">
              Fill in only what you want: personal details, experience, skills, links or just a resume
              website.
            </span>
          </button>
        </div>
      ) : (
        <div className="profile">
          <ProfileOverview profile={current} documents={documents} />
          <PersonalSection profile={current} update={update} />
          <ProfessionalSection profile={current} update={update} />
          <ExperienceSection profile={current} update={update} />
          <EducationSection profile={current} update={update} />
          <LanguagesSection profile={current} update={update} />
          <LinksSection profile={current} update={update} />
          <DocumentsSection documents={documents} onImport={(d) => void importDocument(d)} importing={importing} />
          <CustomFieldsSection
            fields={current.customFields}
            documents={documents}
            onChange={(customFields) => update({ customFields })}
          />
        </div>
      )}

      {dirty && (
        <div className="save-bar" role="region" aria-label="Unsaved changes">
          {error ? (
            <span className="form-error">{error}</span>
          ) : (
            <span className="save-bar__text">Unsaved changes</span>
          )}
          <button
            type="button"
            className="button button--ghost"
            disabled={saving}
            onClick={() => {
              setDraft(null);
              setError(null);
            }}
          >
            Discard
          </button>
          <button
            type="button"
            className="button button--primary"
            disabled={saving}
            onClick={() => void save(current)}
          >
            {saving ? 'Saving…' : 'Save profile'}
          </button>
        </div>
      )}

      {review && (
        <ImportReviewDialog
          current={current}
          result={review}
          onClose={() => setReview(null)}
          onApply={(profile) => {
            setReview(null);
            setBuilding(true);
            void save(profile).then(
              (ok) => ok && setNotice(`Added details from “${review.document.name}”. Everything stays editable.`),
            );
          }}
        />
      )}
    </PageContainer>
  );
}
