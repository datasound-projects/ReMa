import { useNavigation } from '../app/navigation';
import { PageContainer } from '../components/layout/PageContainer';
import { PortfolioStudio } from '../components/portfolio/PortfolioStudio';
import { useProfile } from '../hooks/useProfile';
import { emptyProfile } from '../services/profileService';

interface PortfolioStudioPageProps {
  /** The CV open in the editor, if any. */
  portfolioId: number | null;
}

/**
 * Portfolio Studio: CVs built in ReMa, each with its own design. A page of
 * its own, next to the Profile it can start from.
 */
export function PortfolioStudioPage({ portfolioId }: PortfolioStudioPageProps) {
  const { navigate } = useNavigation();
  const loaded = useProfile();
  const profile = loaded.state.status === 'success' ? loaded.state.data.profile : null;
  const profileAvailable = profile !== null && JSON.stringify(profile) !== JSON.stringify(emptyProfile());

  return (
    <PageContainer
      title="Portfolio Studio"
      subtitle="Build polished CVs from your Profile and export them as PDF."
      width={portfolioId !== null ? 'studio' : 'default'}
    >
      <PortfolioStudio
        openId={portfolioId}
        onOpen={(id) => navigate({ page: 'portfolio', portfolioId: id })}
        profileAvailable={profileAvailable}
      />
    </PageContainer>
  );
}
