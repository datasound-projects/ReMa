// @vitest-environment jsdom
import '../../test/dom';

import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { NetworkContactsCard } from './NetworkContactsCard';

const mocks = vi.hoisted(() => ({
  getNetworkContacts: vi.fn(),
  importNetworkContacts: vi.fn(),
  clearNetworkContacts: vi.fn(),
}));
vi.mock('../../services/networkService', () => mocks);
vi.mock('../../services/systemService', () => ({ openExternalUrl: vi.fn().mockResolvedValue(null) }));

beforeEach(() => {
  vi.clearAllMocks();
  mocks.getNetworkContacts.mockResolvedValue({ linkedin: 0, vcard: 0, lastImportedAt: null });
});

describe('Settings → Connectors → Your contacts', () => {
  it('imports the LinkedIn export and says what was read', async () => {
    mocks.importNetworkContacts.mockResolvedValue({
      read: 412,
      problems: ['notes.csv: It is not LinkedIn’s Connections.csv (no “First Name” column).'],
      summary: { linkedin: 412, vcard: 0, lastImportedAt: 1 },
    });
    render(<NetworkContactsCard />);
    expect(await screen.findByText('No contacts imported yet.')).toBeTruthy();

    fireEvent.click(screen.getByRole('button', { name: 'Import LinkedIn connections…' }));
    await waitFor(() => expect(mocks.importNetworkContacts).toHaveBeenCalledWith('linkedin_export'));
    expect(await screen.findByText('412 LinkedIn connections')).toBeTruthy();
    expect(screen.getByText(/Imported 412 contacts\. notes\.csv:/)).toBeTruthy();
  });

  it('imports vCards, keeps quiet when the picker is cancelled and removes contacts', async () => {
    mocks.getNetworkContacts.mockResolvedValue({ linkedin: 3, vcard: 1, lastImportedAt: 1 });
    mocks.importNetworkContacts.mockResolvedValue(null);
    mocks.clearNetworkContacts.mockResolvedValue({ linkedin: 0, vcard: 0, lastImportedAt: null });
    render(<NetworkContactsCard />);
    expect(await screen.findByText('3 LinkedIn connections · 1 contact from vCards')).toBeTruthy();

    fireEvent.click(screen.getByRole('button', { name: 'Import XING or other contacts (vCard)…' }));
    await waitFor(() => expect(mocks.importNetworkContacts).toHaveBeenCalledWith('vcard'));
    expect(screen.queryByText(/Imported/)).toBeNull();

    fireEvent.click(screen.getByRole('button', { name: 'Remove imported contacts' }));
    expect(await screen.findByText('Imported contacts were removed.')).toBeTruthy();
    expect(mocks.clearNetworkContacts).toHaveBeenCalledWith(null);
    expect(screen.getByText('No contacts imported yet.')).toBeTruthy();
  });
});
