// @vitest-environment jsdom
import '../../test/dom';

import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { SAMPLE_CONTENT } from '../../lib/portfolio/sample';
import type { AiProposal, PortfolioDocument, PortfolioInput } from '../../services/portfolioService';
import { PortfolioEditor } from './PortfolioEditor';

const mocks = vi.hoisted(() => ({
  savePortfolio: vi.fn(),
  portfolioAiAssist: vi.fn(),
  getProfile: vi.fn(),
}));

vi.mock('../../services/portfolioService', async (original) => ({
  ...(await original<typeof import('../../services/portfolioService')>()),
  savePortfolio: mocks.savePortfolio,
  portfolioAiAssist: mocks.portfolioAiAssist,
}));
vi.mock('../../services/profileService', async (original) => ({
  ...(await original<typeof import('../../services/profileService')>()),
  getProfile: mocks.getProfile,
}));
vi.mock('../../lib/portfolio/thumbnails', async (original) => ({
  ...(await original<typeof import('../../lib/portfolio/thumbnails')>()),
  templateThumbnail: () => Promise.resolve('data:image/png;base64,'),
  documentThumbnail: () => Promise.resolve('data:image/png;base64,'),
}));

const doc = (): PortfolioDocument => ({
  id: 7,
  name: 'Data CV',
  kind: 'cv',
  templateId: 'campaign-modern',
  pageSize: 'a4',
  accent: '',
  style: {},
  content: structuredClone(SAMPLE_CONTENT),
  letter: {},
  sourceDocumentId: null,
  createdAt: 1_700_000_000_000,
  updatedAt: 1_700_000_000_000,
});

const lastSaved = (): PortfolioInput => mocks.savePortfolio.mock.calls.at(-1)?.[1] as PortfolioInput;

beforeEach(async () => {
  vi.clearAllMocks();
  localStorage.clear();
  mocks.savePortfolio.mockImplementation((_id: number, input: PortfolioInput) => Promise.resolve({ ...doc(), ...input, updatedAt: Date.now() }));
  const { emptyProfile } = await vi.importActual<typeof import('../../services/profileService')>('../../services/profileService');
  mocks.getProfile.mockResolvedValue({ profile: emptyProfile(), documents: [], credentials: [], updatedAt: null });
});

const renderEditor = () => {
  render(<PortfolioEditor document={doc()} profileAvailable={false} onClose={() => {}} onOpen={() => {}} />);
  return within(screen.getByLabelText('Document'));
};

describe('Portfolio Studio editor', () => {
  it('shows the document on an editable canvas and saves edits made there', async () => {
    const canvas = renderEditor();
    expect(screen.getByRole('textbox', { name: 'Document name' })).toHaveProperty('value', 'Data CV');
    const name = canvas.getByRole('textbox', { name: 'Full name' });
    expect(name.textContent).toBe('Alex Morgan');
    expect(canvas.getByRole('textbox', { name: 'Experience title' }).textContent).toBe('Experience');
    expect(canvas.getAllByRole('textbox', { name: 'Description' }).length).toBeGreaterThan(0);

    name.textContent = 'Alex M. Morgan';
    fireEvent.input(name);
    await waitFor(() => expect(mocks.savePortfolio).toHaveBeenCalled(), { timeout: 3000 });
    expect(lastSaved().content.header.fullName).toBe('Alex M. Morgan');
    expect(await screen.findByText('Saved')).toBeTruthy();
    // The form view shows the same model.
    const panel = within(screen.getByRole('tabpanel'));
    expect(panel.getByLabelText('Full name')).toHaveProperty('value', 'Alex M. Morgan');
    fireEvent.change(panel.getByLabelText('Headline'), { target: { value: 'Staff Engineer' } });
    expect(canvas.getByRole('textbox', { name: 'Headline' }).textContent).toBe('Staff Engineer');
  });

  it('section actions on the canvas work and can be undone', async () => {
    const canvas = renderEditor();
    const experience = canvas.getByRole('region', { name: 'Experience section' });
    fireEvent.click(within(experience).getByRole('button', { name: 'Hide section' }));
    expect(within(experience).getByText('Hidden · not exported')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Undo' }));
    expect(within(experience).queryByText('Hidden · not exported')).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Redo' }));
    expect(within(experience).getByText('Hidden · not exported')).toBeTruthy();

    fireEvent.click(within(experience).getByRole('button', { name: 'Move section down' }));
    await waitFor(() => expect(mocks.savePortfolio).toHaveBeenCalled(), { timeout: 3000 });
    const kinds = lastSaved().content.sections.map((s) => s.kind);
    expect(kinds.indexOf('experience')).toBeGreaterThan(kinds.indexOf('education'));
    expect(lastSaved().content.sections.find((s) => s.kind === 'experience')?.visible).toBe(false);
  });

  it('design choices change the saved style and can be reset', async () => {
    renderEditor();
    fireEvent.click(screen.getByRole('tab', { name: 'Design' }));
    fireEvent.click(within(screen.getByRole('radiogroup', { name: 'Palette' })).getByRole('radio', { name: 'Wine' }));
    fireEvent.click(screen.getByRole('radio', { name: 'US Letter' }));
    fireEvent.click(within(screen.getByRole('radiogroup', { name: 'Line spacing' })).getByRole('radio', { name: 'Relaxed' }));
    await waitFor(() => expect(lastSaved()?.style?.palette).toBe('wine'), { timeout: 3000 });
    expect(lastSaved().pageSize).toBe('letter');
    expect(lastSaved().style?.lineSpacing).toBe('relaxed');
    fireEvent.click(screen.getByRole('button', { name: 'Reset Style' }));
    await waitFor(() => expect(lastSaved().style).toEqual({}), { timeout: 3000 });
    expect(lastSaved().pageSize).toBe('letter');
  });

  it('offers to restore an unsaved draft from an earlier session', async () => {
    localStorage.setItem('rema.portfolio.draft.7', JSON.stringify({ savedAt: Date.now(), input: { ...doc(), name: 'Recovered name' } }));
    renderEditor();
    const dialog = screen.getByRole('alertdialog', { name: 'Recovered changes' });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Restore' }));
    expect(screen.getByRole('textbox', { name: 'Document name' })).toHaveProperty('value', 'Recovered name');
    await waitFor(() => expect(lastSaved()?.name).toBe('Recovered name'), { timeout: 3000 });
    expect(localStorage.getItem('rema.portfolio.draft.7')).toBeNull();
  });

  it('AI proposals for a section are reviewed, accepted and undone', async () => {
    const proposal: AiProposal = {
      scope: 'section',
      text: null,
      section: {
        ...SAMPLE_CONTENT.sections[1]!,
        entries: [{ ...SAMPLE_CONTENT.sections[1]!.entries[0]!, description: '- Led the platform to 2 billion events a day' }],
      },
      content: null,
      letter: null,
      notes: ['Add the team size'],
      warnings: [],
      model: 'model-a',
    };
    mocks.portfolioAiAssist.mockResolvedValue(proposal);
    const canvas = renderEditor();
    const experience = canvas.getByRole('region', { name: 'Experience section' });
    fireEvent.mouseDown(experience);
    fireEvent.click(screen.getByRole('tab', { name: 'AI Assistant' }));
    fireEvent.click(screen.getByRole('radio', { name: 'Section' }));
    fireEvent.click(screen.getByRole('radio', { name: /Achievement bullets/ }));
    fireEvent.click(screen.getByRole('button', { name: 'Propose' }));
    const region = await screen.findByRole('region', { name: 'Proposal' });
    expect(within(region).getByText('Add the team size')).toBeTruthy();
    expect(mocks.portfolioAiAssist.mock.calls[0]?.[0]).toMatchObject({ action: 'achievements', scope: 'section', sectionId: 'experience' });
    fireEvent.click(within(region).getByRole('button', { name: 'Accept' }));
    await waitFor(() => expect(lastSaved()?.content.sections[1]?.entries).toHaveLength(1), { timeout: 3000 });
    expect(lastSaved().content.sections[1]?.entries[0]?.description).toContain('2 billion');
    await act(async () => {
      fireEvent.click(within(screen.getByText(/^Applied:/).parentElement as HTMLElement).getByRole('button', { name: 'Undo' }));
    });
    await waitFor(() => expect(lastSaved()?.content.sections[1]?.entries).toHaveLength(2), { timeout: 3000 });
  });
});
