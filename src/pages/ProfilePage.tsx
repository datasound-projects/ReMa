import { useState } from 'react';

import { useNavigation, type ProfileSection } from '../app/navigation';
import { PageContainer } from '../components/layout/PageContainer';
import { CustomProfileTab } from '../components/profile/CustomProfileTab';
import { DocumentsTab } from '../components/profile/documents/DocumentsTab';
import { LoadingState } from '../components/ui/EmptyState';
import { useProfile } from '../hooks/useProfile';
import { toApiError } from '../services/ipc';
import { emptyProfile, saveProfile, type Profile } from '../services/profileService';

const sameProfile = (a: Profile, b: Profile) => JSON.stringify(a) === JSON.stringify(b);

const PROFILE_TABS: { id: ProfileSection; label: string; description: string }[] = [
  {
    id: 'documents',
    label: 'Documents & Credentials',
    description: 'Historical and professional source material: your CVs, and credentials as supporting evidence.',
  },
  {
    id: 'custom',
    label: 'Custom Profile',
    description: 'Your current career context: goals, preferences and facts you define yourself.',
  },
];

/** How Chat's Profile switch uses the two parts (one fixed rule). */
const PROFILE_CONTEXT_NOTE =
  'When Profile is enabled in Chat, ReMa combines your Custom Profile, CVs and credentials into one career ' +
  'context. Your Custom Profile is treated as your current information and takes priority when it conflicts ' +
  'with uploaded documents.';

interface ProfilePageProps {
  section: ProfileSection;
}

/**
 * The user's career context in two parts. The selected part lives in the
 * navigation state, so nothing (an upload, a preview, a re-render)
 * switches it except the user.
 */
export function ProfilePage({ section }: ProfilePageProps) {
  const { navigate } = useNavigation();
  const loaded = useProfile();
  const view = loaded.state.status === 'success' ? loaded.state.data : null;
  const saved = view?.profile ?? null;

  // The Custom Profile draft follows the saved profile until edited; it is
  // kept while switching between the Profile's parts.
  const [draft, setDraft] = useState<Profile | null>(null);
  const current = draft ?? saved ?? emptyProfile();
  const dirty = draft !== null && saved !== null && !sameProfile(draft, saved);
  const update = (patch: Partial<Profile>) => setDraft({ ...current, ...patch });
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);

  const save = async (profile: Profile) => {
    setSaving(true);
    setSaveError(null);
    try {
      await saveProfile(profile);
      setDraft(null);
      loaded.refresh();
      return true;
    } catch (err) {
      setDraft(profile);
      setSaveError(toApiError(err).message);
      return false;
    } finally {
      setSaving(false);
    }
  };

  const select = (next: ProfileSection) => navigate({ page: 'profile', section: next });
  const selected = PROFILE_TABS.find((tab) => tab.id === section) ?? PROFILE_TABS[0];

  return (
    <PageContainer
      title="Profile"
      subtitle={
        <>
          Your career context for ReMa.
          <span className="page__subtitle-note">{PROFILE_CONTEXT_NOTE}</span>
        </>
      }
    >
      <div className="profile-tabs" role="tablist" aria-label="Profile">
        {PROFILE_TABS.map((tab) => {
          const active = tab.id === section;
          return (
            <button
              key={tab.id}
              type="button"
              role="tab"
              id={`profile-tab-${tab.id}`}
              aria-selected={active}
              aria-controls={`profile-panel-${tab.id}`}
              className={active ? 'profile-tabs__tab profile-tabs__tab--active' : 'profile-tabs__tab'}
              title={tab.description}
              onClick={() => (active ? undefined : select(tab.id))}
            >
              {tab.label}
              {tab.id === 'custom' && dirty && (
                <span className="profile-tabs__dot" title="Unsaved changes" aria-label="Unsaved changes" />
              )}
            </button>
          );
        })}
      </div>

      <div
        role="tabpanel"
        id={`profile-panel-${section}`}
        aria-labelledby={`profile-tab-${section}`}
        className="profile-panel"
      >
        <p className="profile-panel__description">{selected?.description}</p>
        {loaded.state.status === 'loading' && <LoadingState label="Loading your Profile…" />}
        {loaded.state.status === 'error' && (
          <div className="notice notice--danger" role="alert">
            {loaded.state.error.message}{' '}
            <button type="button" className="link-button" onClick={loaded.retry}>
              Try again
            </button>
          </div>
        )}
        {view && section === 'documents' && <DocumentsTab view={view} />}
        {view && section === 'custom' && (
          <CustomProfileTab view={view} current={current} update={update} save={save} />
        )}
      </div>

      {dirty && section === 'custom' && (
        <div className="save-bar" role="region" aria-label="Unsaved changes">
          {saveError ? (
            <span className="form-error">{saveError}</span>
          ) : (
            <span className="save-bar__text">Unsaved changes</span>
          )}
          <button
            type="button"
            className="button button--ghost"
            disabled={saving}
            onClick={() => {
              setDraft(null);
              setSaveError(null);
            }}
          >
            Discard
          </button>
          <button type="button" className="button button--primary" disabled={saving} onClick={() => void save(current)}>
            {saving ? 'Saving…' : 'Save profile'}
          </button>
        </div>
      )}
    </PageContainer>
  );
}
