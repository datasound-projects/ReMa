import { useState } from 'react';

import { useNavigation, type BusinessTab } from '../app/navigation';
import { BusinessProfileView } from '../components/business/BusinessProfileView';
import { FindClients } from '../components/business/FindClients';
import { FindContracts } from '../components/business/FindContracts';
import { GtmStudio } from '../components/business/GtmStudio';
import { Pipeline } from '../components/business/Pipeline';
import { PageContainer } from '../components/layout/PageContainer';
import { LoadingState } from '../components/ui/EmptyState';
import { useBusinessOverview } from '../hooks/useBusiness';
import type { Offer } from '../services/businessService';

const TABS: { id: BusinessTab; label: string; description: string }[] = [
  { id: 'clients', label: 'Find Clients', description: 'Organizations that may buy what you offer.' },
  { id: 'contracts', label: 'Find Contract Work', description: 'Advertised freelance, consulting and project engagements.' },
  { id: 'gtm', label: 'Go-to-Market Studio', description: 'Segments, positioning, channels, drafts and experiments.' },
  { id: 'pipeline', label: 'Pipeline', description: 'Your saved commercial opportunities and what happened.' },
];

const OFFER_KEY = 'rema.business.activeOffer';

function remembered(): string | null {
  try {
    return window.localStorage.getItem(OFFER_KEY);
  } catch {
    return null;
  }
}

function remember(id: string) {
  try {
    window.localStorage.setItem(OFFER_KEY, id);
  } catch {
    // A convenience only.
  }
}

/** The offer research uses by default: the remembered one, else the first reviewed one. */
function pickOffer(offers: Offer[], chosen: string | null): Offer | null {
  const usable = offers.filter((o) => !o.archived);
  return (
    offers.find((o) => o.id === chosen) ??
    usable.find((o) => o.currentVersion !== null) ??
    usable[0] ??
    null
  );
}

/**
 * Business: find customers and contract work for what the user offers, plan
 * how to reach buyers, and keep one commercial pipeline — separate from job
 * Applications (B1).
 */
export function BusinessPage({ tab, opportunityId }: { tab: BusinessTab; opportunityId: string | null }) {
  const { navigate } = useNavigation();
  const overview = useBusinessOverview();
  const [chosenOffer, setChosenOffer] = useState<string | null>(remembered);
  const [profile, setProfile] = useState<{ offerId: string | null } | null>(null);

  const data = overview.state.status === 'success' ? overview.state.data : null;
  const offer = data ? pickOffer(data.offers, chosenOffer) : null;
  const selectTab = (next: BusinessTab) => navigate({ page: 'business', tab: next, opportunityId: null });
  const openOpportunity = (id: string | null) => navigate({ page: 'business', tab: 'pipeline', opportunityId: id });
  const editOffer = () => setProfile({ offerId: offer?.id ?? null });

  return (
    <PageContainer
      title="Business"
      subtitle="Find customers and contract opportunities for what you offer."
      width="wide"
      actions={
        !profile && (
          <button type="button" className="button button--secondary" onClick={() => setProfile({ offerId: null })}>
            Business Profile
          </button>
        )
      }
    >
      {overview.state.status === 'loading' && <LoadingState label="Loading Business…" />}
      {overview.state.status === 'error' && (
        <div className="notice notice--danger" role="alert">
          {overview.state.error.message}{' '}
          <button type="button" className="link-button" onClick={overview.retry}>
            Try again
          </button>
        </div>
      )}
      {data && profile && (
        <BusinessProfileView overview={data} initialOfferId={profile.offerId} onBack={() => setProfile(null)} />
      )}
      {data && !profile && (
        <div className="biz-body">
          <div className="profile-tabs biz-tabs" role="tablist" aria-label="Business">
            {TABS.map((t) => (
              <button
                key={t.id}
                type="button"
                role="tab"
                id={`business-tab-${t.id}`}
                aria-selected={tab === t.id}
                aria-controls={`business-panel-${t.id}`}
                title={t.description}
                className={tab === t.id ? 'profile-tabs__tab profile-tabs__tab--active' : 'profile-tabs__tab'}
                onClick={() => tab !== t.id && selectTab(t.id)}
              >
                {t.label}
              </button>
            ))}
          </div>

          {(tab === 'clients' || tab === 'gtm') && (
            <div className="biz-offer-bar">
              <span className="biz-offer-bar__label">Active offer</span>
              {data.offers.filter((o) => !o.archived).length > 0 ? (
                <select
                  className="input biz-offer-bar__select"
                  aria-label="Active offer"
                  value={offer?.id ?? ''}
                  onChange={(e) => {
                    setChosenOffer(e.target.value);
                    remember(e.target.value);
                  }}
                >
                  {data.offers
                    .filter((o) => !o.archived || o.id === offer?.id)
                    .map((o) => (
                      <option key={o.id} value={o.id}>
                        {o.name}
                        {o.currentVersion !== null ? ` v${o.currentVersion}` : ' (not reviewed)'}
                      </option>
                    ))}
                </select>
              ) : (
                <span className="biz-muted">No offer yet</span>
              )}
              <button type="button" className="link-button" onClick={editOffer}>
                {offer ? 'Change offer' : 'Describe what you offer'}
              </button>
            </div>
          )}

          <div role="tabpanel" id={`business-panel-${tab}`} aria-labelledby={`business-tab-${tab}`} className="biz-panel">
            {tab === 'clients' && <FindClients offer={offer} onEditOffer={editOffer} onOpenOpportunity={openOpportunity} />}
            {tab === 'contracts' && <FindContracts onOpenOpportunity={openOpportunity} />}
            {tab === 'gtm' && (
              <GtmStudio overview={data} offer={offer} onEditOffer={editOffer} onOpenOpportunity={openOpportunity} />
            )}
            {tab === 'pipeline' && <Pipeline overview={data} focusId={opportunityId} onFocus={openOpportunity} />}
          </div>
        </div>
      )}
    </PageContainer>
  );
}
