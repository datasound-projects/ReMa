import { useState } from 'react';

import { toApiError } from '../../services/ipc';
import {
  addProfileDocument,
  importProfileDocument,
  type Profile,
  type ProfileDocument,
  type ProfileImport,
  type ProfileView,
} from '../../services/profileService';
import { ChevronDownIcon, FileIcon, UploadIcon } from '../icons';
import { Menu } from '../ui/Menu';
import { CustomFieldsSection } from './CustomFieldsSection';
import { ImportReviewDialog } from './ImportReviewDialog';
import { ProfileOverview } from './ProfileOverview';
import {
  EducationSection,
  ExperienceSection,
  LanguagesSection,
  LinksSection,
  PersonalSection,
  ProfessionalSection,
  SkillsSection,
} from './ProfileSections';

interface CustomProfileTabProps {
  view: ProfileView;
  /** The draft being edited (the saved Custom Profile until changed). */
  current: Profile;
  update: (patch: Partial<Profile>) => void;
  /** Saves the whole profile; resolves to whether it worked. */
  save: (profile: Profile) => Promise<boolean>;
}

/**
 * The optional, structured Custom Profile. It is filled only by the user:
 * reading details from a CV is a separate, explicit action whose results
 * are reviewed before anything is saved.
 */
export function CustomProfileTab({ view, current, update, save }: CustomProfileTabProps) {
  const [reading, setReading] = useState<number | null>(null);
  const [review, setReview] = useState<ProfileImport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const readable = view.documents.filter((d) => d.kind === 'cv' && d.hasText);

  const readCv = async (doc: ProfileDocument) => {
    setReading(doc.id);
    setError(null);
    setNotice(null);
    try {
      setReview(await importProfileDocument(doc.id));
    } catch (err) {
      setError(toApiError(err).message);
    } finally {
      setReading(null);
    }
  };

  const uploadAndRead = async () => {
    setError(null);
    try {
      const doc = await addProfileDocument('cv');
      if (doc) await readCv(doc);
    } catch (err) {
      setError(toApiError(err).message);
    }
  };

  return (
    <div className="profile">
      <div className="profile-intro">
        <p className="profile-intro__text">
          Optional. Fill in only what helps: empty fields are fine, and this stays separate from your uploaded
          documents.
        </p>
        <Menu
          trigger={(props) => (
            <button type="button" className="button button--secondary" disabled={reading !== null} {...props}>
              <FileIcon className="button__icon" />
              {reading !== null ? 'Reading…' : 'Fill from a CV…'}
              <ChevronDownIcon className="button__icon" />
            </button>
          )}
        >
          {(close) => (
            <div className="fill-menu">
              <p className="fill-menu__hint">ReMa reads the CV and shows what it found. Nothing is saved until you choose.</p>
              {readable.map((doc) => (
                <button
                  key={doc.id}
                  type="button"
                  role="menuitem"
                  className="menu__item"
                  onClick={() => {
                    close();
                    void readCv(doc);
                  }}
                >
                  {doc.name}
                  {doc.isPrimary && <span className="fill-menu__tag">Primary</span>}
                </button>
              ))}
              {readable.length === 0 && <p className="fill-menu__empty">No CV with readable text yet.</p>}
              <button
                type="button"
                role="menuitem"
                className="menu__item"
                onClick={() => {
                  close();
                  void uploadAndRead();
                }}
              >
                <UploadIcon className="button__icon" />
                Upload a CV and read it…
              </button>
            </div>
          )}
        </Menu>
      </div>
      {error && (
        <p className="form-error" role="alert">
          {error}
        </p>
      )}
      {notice && (
        <p className="notice" role="status">
          {notice}
        </p>
      )}

      <ProfileOverview profile={current} />
      <PersonalSection profile={current} update={update} />
      <ProfessionalSection profile={current} update={update} />
      <ExperienceSection profile={current} update={update} />
      <EducationSection profile={current} update={update} />
      <SkillsSection profile={current} update={update} />
      <LanguagesSection profile={current} update={update} />
      <LinksSection profile={current} update={update} />
      <CustomFieldsSection
        fields={current.customFields}
        documents={view.documents}
        onChange={(customFields) => update({ customFields })}
      />

      {review && (
        <ImportReviewDialog
          current={current}
          result={review}
          onClose={() => setReview(null)}
          onApply={(profile) => {
            const name = review.document.name;
            setReview(null);
            void save(profile).then((ok) => ok && setNotice(`Added the details you chose from “${name}”.`));
          }}
        />
      )}
    </div>
  );
}
