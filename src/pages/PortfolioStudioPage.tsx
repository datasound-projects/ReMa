import { useNavigation } from '../app/navigation';
import { PageContainer } from '../components/layout/PageContainer';
import { PortfolioStudio } from '../components/portfolio/PortfolioStudio';
import { useProfile } from '../hooks/useProfile';
import { emptyProfile } from '../services/profileService';

interface PortfolioStudioPageProps {
  /** The document open in the editor, if any. */
  portfolioId: number | null;
}

/**
 * Portfolio Studio: CVs and cover letters built in ReMa, each with its own
 * design. A page of its own, next to the Profile it can start from. The
 * editor takes the whole page; the document list has the usual header.
 */
export function PortfolioStudioPage({ portfolioId }: PortfolioStudioPageProps) {
  const { navigate } = useNavigation();
  const loaded = useProfile();
  const profile = loaded.state.status === 'success' ? loaded.state.data.profile : null;
  const profileAvailable = profile !== null && JSON.stringify(profile) !== JSON.stringify(emptyProfile());
  const studio = (
    <PortfolioStudio openId={portfolioId} onOpen={(id) => navigate({ page: 'portfolio', portfolioId: id })} profileAvailable={profileAvailable} />
  );

  if (portfolioId !== null) {
    return (
      <div className="page page--studio">
        <div className="page__inner page__inner--studio">{studio}</div>
      </div>
    );
  }
  return (
    <PageContainer title="Portfolio Studio" subtitle="CVs and cover letters with their own design, exported as PDF." width="wide">
      {studio}
    </PageContainer>
  );
}
