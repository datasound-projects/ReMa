// @vitest-environment jsdom
import '../test/dom';

import { fireEvent, render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { NavigationContext, type View } from '../app/navigation';
import type { PortfolioDocument } from '../services/portfolioService';
import { PortfolioStudioPage } from './PortfolioStudioPage';

const mocks = vi.hoisted(() => ({
  listPortfolios: vi.fn(),
  getProfile: vi.fn(),
}));

vi.mock('../services/portfolioService', async (original) => ({
  ...(await original<typeof import('../services/portfolioService')>()),
  listPortfolios: mocks.listPortfolios,
}));

vi.mock('../services/profileService', async (original) => ({
  ...(await original<typeof import('../services/profileService')>()),
  getProfile: mocks.getProfile,
}));

const cv = (id: number, name: string): PortfolioDocument =>
  ({
    id,
    name,
    templateId: 'modern',
    pageSize: 'a4',
    accent: '',
    createdAt: 1_700_000_000_000,
    updatedAt: 1_700_000_000_000,
  }) as unknown as PortfolioDocument;

beforeEach(async () => {
  vi.clearAllMocks();
  const { emptyProfile } = await vi.importActual<typeof import('../services/profileService')>('../services/profileService');
  mocks.getProfile.mockResolvedValue({ profile: emptyProfile(), documents: [], credentials: [], updatedAt: null });
  mocks.listPortfolios.mockResolvedValue([cv(3, 'Data CV')]);
});

describe('Portfolio Studio page', () => {
  it('is its own page and opens CVs through navigation', async () => {
    const navigate = vi.fn<(view: View) => void>();
    render(
      <NavigationContext value={{ view: { page: 'portfolio', portfolioId: null }, navigate }}>
        <PortfolioStudioPage portfolioId={null} />
      </NavigationContext>,
    );
    expect(screen.getByRole('heading', { level: 1, name: 'Portfolio Studio' })).toBeTruthy();
    expect(screen.queryByRole('tab')).toBeNull();
    fireEvent.click(await screen.findByRole('button', { name: 'Data CV' }));
    expect(navigate).toHaveBeenCalledWith({ page: 'portfolio', portfolioId: 3 });
  });
});
