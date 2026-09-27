// @vitest-environment jsdom
import '../../test/dom';

import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { AdditionalSearchService } from './AdditionalSearchService';

const mocks = vi.hoisted(() => ({
  webSearchSettings: vi.fn(),
  saveWebSearchSettings: vi.fn(),
  testWebSearch: vi.fn(),
}));

vi.mock('../../services/webSearchService', () => mocks);

beforeEach(() => {
  vi.clearAllMocks();
  mocks.webSearchSettings.mockResolvedValue({ service: null, url: null, hasKey: false });
});

describe('Settings → Career Search → Advanced: optional search service', () => {
  it('saves a service with its key and never shows the stored key', async () => {
    mocks.saveWebSearchSettings.mockResolvedValue({ service: 'brave', url: null, hasKey: true });
    mocks.testWebSearch.mockResolvedValue({ results: 8, titles: ['AI Engineer – Vienna'] });
    render(<AdditionalSearchService />);
    const select = await screen.findByLabelText('Search service');
    await waitFor(() => expect((select as HTMLSelectElement).disabled).toBe(false));
    expect(screen.getByRole('button', { name: 'Test' })).toHaveProperty('disabled', true);

    fireEvent.change(select, { target: { value: 'brave' } });
    const key = screen.getByLabelText('API key') as HTMLInputElement;
    expect(key.type).toBe('password');
    fireEvent.change(key, { target: { value: 'secret-key' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    await waitFor(() =>
      expect(mocks.saveWebSearchSettings).toHaveBeenCalledWith({ service: 'brave', url: null, key: 'secret-key' }),
    );
    expect(await screen.findByText('Saved.')).toBeTruthy();
    expect((screen.getByLabelText('API key') as HTMLInputElement).value).toBe('');
    expect(screen.getByPlaceholderText('•••••••• (unchanged)')).toBeTruthy();

    fireEvent.click(screen.getByRole('button', { name: 'Test' }));
    expect(await screen.findByText('Works: 8 results (e.g. “AI Engineer – Vienna”).')).toBeTruthy();
  });

  it('asks for a SearXNG address and shows why a test failed', async () => {
    mocks.webSearchSettings.mockResolvedValue({ service: 'searxng', url: 'http://localhost:8888', hasKey: false });
    mocks.testWebSearch.mockRejectedValue({ code: 'provider', message: 'SearXNG refused JSON results.' });
    render(<AdditionalSearchService />);
    const address = (await screen.findByLabelText('Address')) as HTMLInputElement;
    await waitFor(() => expect(address.value).toBe('http://localhost:8888'));
    expect(screen.queryByLabelText('API key')).toBeNull();

    fireEvent.click(screen.getByRole('button', { name: 'Test' }));
    expect((await screen.findByRole('alert')).textContent).toBe('SearXNG refused JSON results.');
  });
});
