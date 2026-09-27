// @vitest-environment jsdom
import '../../test/dom';

import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { RemaMcpStatus } from '../../services/remaMcpService';
import { RemaMcpCard } from './RemaMcpCard';

const mocks = vi.hoisted(() => ({
  remaMcpStatus: vi.fn(),
  setRemaMcpEnabled: vi.fn(),
  clearRemaMcpCache: vi.fn(),
}));

vi.mock('../../services/remaMcpService', () => mocks);

const on: RemaMcpStatus = {
  enabled: true,
  readiness: 'ready',
  message: 'Searches with SearXNG.',
  backend: 'SearXNG',
  tools: [],
  sources: [
    { name: 'Greenhouse', access: 'Public API / feed', usable: true, note: 'Published jobs only.', lastError: null },
    { name: 'LinkedIn', access: 'Links only', usable: true, note: 'Discovered links only.', lastError: null },
  ],
  cachedJobs: 3,
  cachedSearches: 1,
};
const off: RemaMcpStatus = { ...on, enabled: false, readiness: 'disabled', message: 'Turned off.' };

const toggle = () => screen.getByRole('switch', { name: 'ReMa MCP enabled' }) as HTMLInputElement;

beforeEach(() => {
  vi.clearAllMocks();
  mocks.remaMcpStatus.mockResolvedValue(on);
  mocks.setRemaMcpEnabled.mockImplementation(async (enabled: boolean) => (enabled ? on : off));
});

async function shown() {
  render(<RemaMcpCard />);
  expect(await screen.findByText('ReMa MCP — Built-in job search')).toBeTruthy();
  await waitFor(() => expect(toggle().checked).toBe(true));
}

describe('Settings → MCP → Built-in: ReMa MCP', () => {
  it('is on by default, shows its readiness, and cannot be edited or removed', async () => {
    await shown();
    expect(screen.getByText('Ready')).toBeTruthy();
    expect(screen.getByText('Job search and job descriptions')).toBeTruthy();
    expect(screen.queryByRole('button', { name: /edit|remove|delete/i })).toBeNull();
    expect(screen.queryByText(/port|\.exe|\/usr\//i)).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Sources' }));
    expect(screen.getByText('Links only')).toBeTruthy();
  });

  it('turns off only after two consecutive confirmations', async () => {
    await shown();
    fireEvent.click(toggle());
    let dialog = screen.getByRole('dialog', { name: 'Disable ReMa MCP?' });
    expect(within(dialog).getByText('Its built-in job-search and job-description tools will become unavailable.')).toBeTruthy();
    fireEvent.click(within(dialog).getByRole('button', { name: 'Continue' }));

    dialog = screen.getByRole('dialog', { name: 'Confirm disabling ReMa MCP?' });
    expect(within(dialog).getByText('You can enable it again anytime in Settings.')).toBeTruthy();
    expect(mocks.setRemaMcpEnabled).not.toHaveBeenCalled();
    fireEvent.click(within(dialog).getByRole('button', { name: 'Disable ReMa MCP' }));

    await waitFor(() => expect(mocks.setRemaMcpEnabled).toHaveBeenCalledWith(false));
    expect(mocks.setRemaMcpEnabled).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(toggle().checked).toBe(false));
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('cancel, keep enabled, Escape or closing either dialog changes nothing', async () => {
    await shown();
    // First warning: Cancel.
    fireEvent.click(toggle());
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(screen.queryByRole('dialog')).toBeNull();
    // First warning: Escape.
    fireEvent.click(toggle());
    fireEvent(screen.getByRole('dialog'), new Event('cancel', { cancelable: true }));
    expect(screen.queryByRole('dialog')).toBeNull();
    // Final confirmation: Keep enabled.
    fireEvent.click(toggle());
    fireEvent.click(screen.getByRole('button', { name: 'Continue' }));
    fireEvent.click(screen.getByRole('button', { name: 'Keep enabled' }));
    expect(screen.queryByRole('dialog')).toBeNull();
    // Final confirmation: Escape, then a click outside.
    fireEvent.click(toggle());
    fireEvent.click(screen.getByRole('button', { name: 'Continue' }));
    fireEvent(screen.getByRole('dialog'), new Event('cancel', { cancelable: true }));
    expect(screen.queryByRole('dialog')).toBeNull();
    fireEvent.click(toggle());
    fireEvent.mouseDown(screen.getByRole('dialog'));
    expect(screen.queryByRole('dialog')).toBeNull();

    expect(mocks.setRemaMcpEnabled).not.toHaveBeenCalled();
    expect(toggle().checked).toBe(true);
  });

  it('turns back on with one click', async () => {
    mocks.remaMcpStatus.mockResolvedValue(off);
    render(<RemaMcpCard />);
    await waitFor(() => expect(toggle().checked).toBe(false));
    expect(screen.getByText('Off')).toBeTruthy();
    fireEvent.click(toggle());
    expect(screen.queryByRole('dialog')).toBeNull();
    await waitFor(() => expect(mocks.setRemaMcpEnabled).toHaveBeenCalledWith(true));
    await waitFor(() => expect(toggle().checked).toBe(true));
  });

  it('shows the newest status from the backend after an action', async () => {
    mocks.remaMcpStatus.mockResolvedValueOnce(off).mockResolvedValue({
      ...on,
      readiness: 'offline',
      message: 'Recent requests to job sources could not connect.',
    });
    render(<RemaMcpCard />);
    await waitFor(() => expect(toggle().checked).toBe(false));
    fireEvent.click(toggle());
    expect(await screen.findByText('Offline')).toBeTruthy();
    expect(toggle().checked).toBe(true);
  });

  it('checks the built-in server over MCP and clears only its cache', async () => {
    mocks.remaMcpStatus.mockImplementation(async (check: boolean) =>
      check ? { ...on, tools: ['search_jobs', 'get_job', 'get_jobs', 'search_similar_jobs', 'source_status'] } : on,
    );
    mocks.clearRemaMcpCache.mockResolvedValue({ ...on, cachedJobs: 0, cachedSearches: 0 });
    await shown();
    fireEvent.click(screen.getByRole('button', { name: 'Check' }));
    expect(await screen.findByText(/Working: 5 tools available/)).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: /Clear cache \(3 jobs\)/ }));
    await waitFor(() => expect(mocks.clearRemaMcpCache).toHaveBeenCalled());
  });
});
