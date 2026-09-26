// @vitest-environment jsdom
import '../test/dom';

import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { NavigationContext, type View } from '../app/navigation';
import type { ProfileCredential, ProfileDocument, ProfileView } from '../services/profileService';
import { ProfilePage } from './ProfilePage';

const doc = (id: number, name: string, patch: Partial<ProfileDocument> = {}): ProfileDocument => ({
  id,
  name,
  kind: 'cv',
  format: 'docx',
  originalName: `${name}.docx`,
  size: 24_000,
  hasText: true,
  isPrimary: false,
  createdAt: 1_700_000_000_000,
  updatedAt: 1_700_000_000_000,
  ...patch,
});

const mocks = vi.hoisted(() => ({
  view: null as unknown as ProfileView,
  getProfile: vi.fn(),
  addProfileDocuments: vi.fn(),
  addProfileDocument: vi.fn(),
  saveProfile: vi.fn(),
  importProfileDocument: vi.fn(),
  profileDocumentBlocks: vi.fn(),
  saveCredential: vi.fn(),
  deleteCredential: vi.fn(),
  deleteProfileDocument: vi.fn(),
  setPrimaryDocument: vi.fn(),
}));

vi.mock('../services/profileService', async (original) => ({
  ...(await original<typeof import('../services/profileService')>()),
  getProfile: mocks.getProfile,
  addProfileDocuments: mocks.addProfileDocuments,
  addProfileDocument: mocks.addProfileDocument,
  saveProfile: mocks.saveProfile,
  importProfileDocument: mocks.importProfileDocument,
  profileDocumentBlocks: mocks.profileDocumentBlocks,
  saveCredential: mocks.saveCredential,
  deleteCredential: mocks.deleteCredential,
  deleteProfileDocument: mocks.deleteProfileDocument,
  setPrimaryDocument: mocks.setPrimaryDocument,
  documentUrl: (id: number) => `rema-doc://localhost/document/${id}`,
}));

const credential: ProfileCredential = {
  id: 7,
  kind: 'professional_certificate',
  title: 'AWS Solutions Architect',
  issuer: 'Amazon Web Services',
  issueDate: '2023-05',
  expirationDate: '',
  credentialId: 'ABC-1',
  credentialUrl: '',
  note: '',
  document: null,
  createdAt: 0,
  updatedAt: 0,
};

function renderProfile(section: 'documents' | 'custom' = 'documents') {
  const navigate = vi.fn<(view: View) => void>();
  const view: View = { page: 'profile', section, portfolioId: null };
  const utils = render(
    <NavigationContext value={{ view, navigate }}>
      <ProfilePage section={section} portfolioId={null} />
    </NavigationContext>,
  );
  return { navigate, ...utils };
}

beforeEach(async () => {
  vi.clearAllMocks();
  const { emptyProfile } = await vi.importActual<typeof import('../services/profileService')>('../services/profileService');
  mocks.view = {
    profile: emptyProfile(),
    documents: [doc(1, 'Resume 2026', { isPrimary: true })],
    credentials: [credential],
    updatedAt: null,
  };
  mocks.getProfile.mockImplementation(() => Promise.resolve(mocks.view));
  mocks.profileDocumentBlocks.mockResolvedValue([{ type: 'paragraph', text: 'Senior data engineer in Vienna.' }]);
});

describe('Profile', () => {
  it('opens on Documents & Credentials with three independent options', async () => {
    renderProfile();
    const tabs = screen.getAllByRole('tab');
    expect(tabs.map((t) => t.textContent)).toEqual(['Documents & Credentials', 'Custom Profile', 'Portfolio Studio']);
    expect(tabs[0]?.getAttribute('aria-selected')).toBe('true');
    expect(screen.getByText('Your career context for ReMa.')).toBeTruthy();
    expect(await screen.findByText('Resume 2026')).toBeTruthy();
    expect(screen.getByText('Primary')).toBeTruthy();
  });

  it('uploading CVs stays on the page and never fills the Custom Profile', async () => {
    const added = doc(2, 'Resume short');
    mocks.addProfileDocuments.mockImplementation(async () => {
      mocks.view = { ...mocks.view, documents: [...mocks.view.documents, added] };
      return { added: [added], failed: [{ name: 'photo.exe', reason: 'This file type is not supported.' }] };
    });
    const { navigate } = renderProfile();
    fireEvent.click(await screen.findByRole('button', { name: /Upload CVs/ }));
    expect(await screen.findByText('Added “Resume short”.')).toBeTruthy();
    expect(screen.getByText('photo.exe')).toBeTruthy();
    expect(mocks.addProfileDocuments).toHaveBeenCalledWith('cv');
    expect(navigate).not.toHaveBeenCalled();
    expect(mocks.importProfileDocument).not.toHaveBeenCalled();
    expect(mocks.saveProfile).not.toHaveBeenCalled();
    expect(screen.getAllByRole('tab')[0]?.getAttribute('aria-selected')).toBe('true');
  });

  it('previews a document in place; closing keeps the section', async () => {
    const { navigate } = renderProfile();
    const card = await screen.findByRole('article', { name: 'Resume 2026' });
    fireEvent.click(within(card).getByRole('button', { name: 'Preview' }));
    const viewer = await screen.findByRole('dialog', { name: 'Preview of Resume 2026' });
    expect(await within(viewer).findByText('Senior data engineer in Vienna.')).toBeTruthy();
    // Text is rendered as text, never as HTML.
    expect(viewer.querySelector('script, iframe')).toBeNull();
    fireEvent.click(within(viewer).getByRole('button', { name: 'Close preview' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    expect(navigate).not.toHaveBeenCalled();
    expect(screen.getAllByRole('tab')[0]?.getAttribute('aria-selected')).toBe('true');
  });

  it('adds and removes credentials with optional details', async () => {
    mocks.saveCredential.mockResolvedValue(credential);
    mocks.deleteCredential.mockResolvedValue(null);
    renderProfile();
    fireEvent.click(await screen.findByRole('button', { name: 'Add credential' }));
    const dialog = await screen.findByRole('dialog', { name: 'Add credential' });
    fireEvent.change(within(dialog).getByLabelText('Title'), { target: { value: 'MSc Computer Science' } });
    fireEvent.change(within(dialog).getByLabelText('Type'), { target: { value: 'degree' } });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Add credential' }));
    await waitFor(() =>
      expect(mocks.saveCredential).toHaveBeenCalledWith(
        null,
        expect.objectContaining({ kind: 'degree', title: 'MSc Computer Science', issuer: '' }),
        null,
      ),
    );

    fireEvent.click(screen.getByRole('button', { name: 'Remove credential' }));
    fireEvent.click(within(screen.getByRole('group', { name: 'Confirm removal' })).getByRole('button', { name: 'Remove' }));
    await waitFor(() => expect(mocks.deleteCredential).toHaveBeenCalledWith(7));
  });

  it('switching parts goes through navigation only', async () => {
    const { navigate } = renderProfile();
    fireEvent.click(screen.getByRole('tab', { name: 'Custom Profile' }));
    expect(navigate).toHaveBeenCalledWith({ page: 'profile', section: 'custom', portfolioId: null });
  });

  it('Custom Profile is optional and reads a CV only when asked', async () => {
    renderProfile('custom');
    expect(await screen.findByText(/Optional\. Fill in only what helps/)).toBeTruthy();
    expect(mocks.importProfileDocument).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: /Fill from a CV/ }));
    fireEvent.click(await screen.findByRole('menuitem', { name: /Resume 2026/ }));
    await waitFor(() => expect(mocks.importProfileDocument).toHaveBeenCalledWith(1));
    // Nothing is saved until the review is confirmed.
    expect(mocks.saveProfile).not.toHaveBeenCalled();
  });
});
