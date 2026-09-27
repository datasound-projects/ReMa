// @vitest-environment jsdom
import '../../test/dom';

import { render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import type { ProviderView } from '../../services/providerService';
import { CustomEndpointRow } from './ProviderRows';

vi.mock('../../services/providerService', () => ({
  disconnectProvider: vi.fn(),
  refreshProviderModels: vi.fn(),
  saveCustomProvider: vi.fn(),
  setModelEnabled: vi.fn(),
}));

const endpoint = (overrides: Partial<ProviderView> = {}): ProviderView => ({
  id: 'custom-1',
  kind: 'openai_compatible',
  name: 'Local',
  baseUrl: 'http://localhost:11434/v1',
  configuredModel: 'llama3.2',
  connectionMethods: ['api_key'],
  connection: 'api_key',
  accountLabel: null,
  status: 'connected',
  statusMessage: null,
  signIn: null,
  configured: true,
  hasCredential: true,
  outOfCredits: false,
  billingUrl: null,
  models: [],
  ...overrides,
});

describe('Settings → a custom endpoint', () => {
  it('says nothing while the endpoint accepts its key', () => {
    render(<CustomEndpointRow provider={endpoint()} />);
    expect(screen.queryByRole('alert')).toBeNull();
  });

  it('says so when the endpoint rejected its key', () => {
    render(<CustomEndpointRow provider={endpoint({ status: 'reauth_required' })} />);
    expect(screen.getByRole('alert').textContent).toBe(
      'Local rejected its API key. Edit the endpoint to change the key.',
    );
  });
});
