import { useState } from 'react';

import { useNavigation, type ProfileSection } from '../app/navigation';
import { PageContainer } from '../components/layout/PageContainer';
import { CustomProfileTab } from '../components/profile/CustomProfileTab';
import { DocumentsTab } from '../components/profile/documents/DocumentsTab';
import { PortfolioStudio } from '../components/portfolio/PortfolioStudio';
import { LoadingState } from '../components/ui/EmptyState';
import { useProfile } from '../hooks/useProfile';
import { toApiError } from '../services/ipc';
import { emptyProfile, saveProfile, type Profile } from '../services/profileService';

const sameProfile = (a: Profile, b: Profile) => JSON.stringify(a) === JSON.stringify(b);

const PROFILE_TABS: { id: ProfileSection; label: string }[] = [
  { id: 'documents', label: 'Documents & Credentials' },
  { id: 'custom', label: 'Custom Profile' },
  { id: 'portfolio', label: 'Portfolio Studio' },
];

interface ProfilePageProps {
  section: ProfileSection;
  /** The Portfolio Studio document open in the editor, if any. */
  portfolioId: number | null;
}

/**
 * The user's career context: three independent parts. The selected part
 * lives in the navigation state, so nothing (an upload, a preview, a
 * re-render) switches it except the user.
 */
export function ProfilePage({ section, portfolioId }: ProfilePageProps) {
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

  const select = (next: ProfileSection) => navigate({ page: 'profile', section: next, portfolioId: null });
  const editing = section === 'portfolio' && portfolioId !== null;

  return (
    <PageContainer
      title="Profile"
      subtitle="Your career context for ReMa."
      width={editing ? 'studio' : 'default'}
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
              onClick={() => (active && !editing ? undefined : select(tab.id))}
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
        {view && section === 'portfolio' && (
          <PortfolioStudio
            openId={portfolioId}
            onOpen={(id) => navigate({ page: 'profile', section: 'portfolio', portfolioId: id })}
            profileAvailable={!sameProfile(view.profile, emptyProfile())}
          />
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
