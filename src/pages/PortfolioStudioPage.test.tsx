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
vi.mock('../lib/portfolio/thumbnails', async (original) => ({
  ...(await original<typeof import('../lib/portfolio/thumbnails')>()),
  templateThumbnail: () => Promise.resolve('data:image/png;base64,'),
  documentThumbnail: () => Promise.resolve('data:image/png;base64,'),
}));

const cv = (id: number, name: string): PortfolioDocument =>
  ({
    id,
    name,
    kind: 'cv',
    templateId: 'modern',
    pageSize: 'a4',
    accent: '',
    content: { header: { fullName: 'Ana', headline: '', email: '', phone: '', location: '', website: '', linkedin: '', github: '' }, sections: [] },
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
    expect((await screen.findAllByRole('tab')).map((t) => t.textContent)).toEqual(['My Documents1', 'CV Templates', 'Cover Letter Templates']);
    for (const name of ['Create CV', 'Create Cover Letter', 'Import Existing Document', 'Start from Profile']) {
      expect(screen.getByRole('button', { name })).toBeTruthy();
    }
    expect(screen.getByRole('button', { name: 'Start from Profile' })).toHaveProperty('disabled', true);
    fireEvent.click(await screen.findByRole('button', { name: 'Data CV' }));
    expect(navigate).toHaveBeenCalledWith({ page: 'portfolio', portfolioId: 3 });
  });

  it('lists the CV and cover letter templates with filters', async () => {
    render(
      <NavigationContext value={{ view: { page: 'portfolio', portfolioId: null }, navigate: vi.fn() }}>
        <PortfolioStudioPage portfolioId={null} />
      </NavigationContext>,
    );
    fireEvent.click(await screen.findByRole('tab', { name: 'CV Templates' }));
    expect(screen.getAllByRole('button', { name: /^Use / })).toHaveLength(20);
    fireEvent.change(screen.getByRole('combobox', { name: 'Category' }), { target: { value: 'Legal & Administration' } });
    expect(screen.getAllByRole('button', { name: /^Use / }).map((b) => b.getAttribute('aria-label'))).toEqual(['Use Formal Counsel', 'Use Administrative Essential']);
    fireEvent.click(screen.getByRole('tab', { name: 'Cover Letter Templates' }));
    expect(screen.getAllByRole('button', { name: /^Use / })).toHaveLength(5);
    fireEvent.click(screen.getByRole('button', { name: 'Use Executive' }));
    expect(screen.getByRole('dialog', { name: 'New cover letter' })).toBeTruthy();
  });
});
