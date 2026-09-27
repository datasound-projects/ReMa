// @vitest-environment jsdom
import '../../test/dom';

import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { ProviderView } from '../../services/providerService';
import { CloudProviderRow } from './CloudProviderRow';

const mocks = vi.hoisted(() => ({
  cancelProviderSignIn: vi.fn(),
  checkProviderConnection: vi.fn(),
  connectProvider: vi.fn(),
  disconnectProvider: vi.fn(),
  setDefaultModel: vi.fn(),
  signOutProvider: vi.fn(),
  startProviderSignIn: vi.fn(),
}));

vi.mock('../../services/providerService', () => mocks);
vi.mock('../../services/systemService', () => ({ openExternalUrl: vi.fn() }));

const openai = (overrides: Partial<ProviderView> = {}): ProviderView => ({
  id: 'openai',
  kind: 'openai',
  name: 'OpenAI',
  baseUrl: null,
  configuredModel: null,
  connectionMethods: ['chatgpt_account', 'api_key'],
  connection: 'api_key',
  accountLabel: null,
  status: 'connected',
  statusMessage: null,
  signIn: null,
  configured: true,
  hasCredential: true,
  outOfCredits: false,
  billingUrl: null,
  models: [{ modelId: 'gpt-5.5', displayName: 'GPT-5.5', enabled: true }],
  ...overrides,
});

beforeEach(() => {
  vi.clearAllMocks();
  mocks.checkProviderConnection.mockResolvedValue(undefined);
});

describe('Settings → a cloud provider connected with an API key', () => {
  it('shows Connected while the provider accepts the key', () => {
    render(<CloudProviderRow provider={openai()} defaultModel={null} />);
    expect(screen.getByText('Connected · API key')).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Replace key' })).toBeNull();
  });

  it('says the key stopped working and offers to replace it', async () => {
    mocks.connectProvider.mockResolvedValue(undefined);
    const provider = openai({
      status: 'reauth_required',
      statusMessage: 'OpenAI rejected the API key. Replace it in Settings.',
    });
    render(<CloudProviderRow provider={provider} defaultModel={null} />);

    expect(screen.getByText('Reauthentication required')).toBeTruthy();
    expect(screen.queryByText(/^Connected/)).toBeNull();
    expect(screen.getByRole('alert').textContent).toBe('OpenAI rejected the API key. Replace it in Settings.');

    fireEvent.click(screen.getByRole('button', { name: 'Replace key' }));
    const key = screen.getByLabelText('OpenAI API key') as HTMLInputElement;
    expect(key.type).toBe('password');
    fireEvent.change(key, { target: { value: 'sk-new' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    await waitFor(() => expect(mocks.connectProvider).toHaveBeenCalledWith('openai', 'sk-new'));
    // Never a browser sign-in for a key problem.
    expect(mocks.startProviderSignIn).not.toHaveBeenCalled();
  });

  it('explains a rejected key even without a message from the provider', () => {
    render(<CloudProviderRow provider={openai({ status: 'reauth_required' })} defaultModel={null} />);
    expect(screen.getByText('OpenAI no longer accepts this API key. Replace it to keep using OpenAI.')).toBeTruthy();
  });

  it('offers a reconnect, not a key, when an account sign-in stopped working', () => {
    render(
      <CloudProviderRow
        provider={openai({ connection: 'chatgpt_account', status: 'reauth_required' })}
        defaultModel={null}
      />,
    );
    expect(screen.getByRole('button', { name: 'Reconnect' })).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Replace key' })).toBeNull();
  });
});
