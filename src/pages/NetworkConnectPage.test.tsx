// @vitest-environment jsdom
import '../test/dom';

import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { NavigationContext } from '../app/navigation';
import type { NetworkResult, ProviderCapabilities } from '../services/networkService';
import { NetworkConnectPage } from './NetworkConnectPage';

const mocks = vi.hoisted(() => ({
  getNetworkCapabilities: vi.fn(),
  researchNetwork: vi.fn(),
  cancelNetworkResearch: vi.fn(() => Promise.resolve(true)),
  getLastNetworkResult: vi.fn(() => Promise.resolve(null)),
  openExternalUrl: vi.fn(() => Promise.resolve(null)),
  connectConnector: vi.fn(),
}));

vi.mock('../services/networkService', () => ({
  getNetworkCapabilities: mocks.getNetworkCapabilities,
  researchNetwork: mocks.researchNetwork,
  cancelNetworkResearch: mocks.cancelNetworkResearch,
  getLastNetworkResult: mocks.getLastNetworkResult,
}));
vi.mock('../services/systemService', () => ({
  openExternalUrl: mocks.openExternalUrl,
  getSystemTimezone: vi.fn(() => Promise.resolve('UTC')),
}));
vi.mock('../services/connectorService', () => ({
  connectConnector: mocks.connectConnector,
  cancelConnectorSignIn: vi.fn(() => Promise.resolve(null)),
  getConnectors: vi.fn(() => new Promise(() => {})),
}));
vi.mock('../services/providerService', () => ({ getModelCatalog: vi.fn(() => new Promise(() => {})) }));
vi.mock('../services/taskService', () => ({ listTasks: vi.fn(() => new Promise(() => {})) }));

const linkedin: ProviderCapabilities = {
  provider: 'linkedin',
  name: 'LinkedIn',
  access: 'connected',
  accountName: 'Ana Example',
  available: [
    { capability: 'authenticate_identity', label: 'Identity', reason: 'Granted when you signed in.' },
    { capability: 'read_self_profile', label: 'Your profile', reason: 'Granted when you signed in.' },
  ],
  unavailable: [
    {
      capability: 'read_first_degree_connections',
      label: 'Connection-list access',
      reason: 'LinkedIn has not granted ReMa access to connection lists.',
    },
    { capability: 'search_people', label: 'People search', reason: 'Not part of LinkedIn’s sign-in.' },
  ],
  summary:
    'LinkedIn is connected for identity, but ReMa does not currently have permission to read your connection list.',
  grantedScopes: ['openid', 'profile', 'email'],
  grantAvailable: false,
};

const xing: ProviderCapabilities = {
  provider: 'xing',
  name: 'XING',
  access: 'not_available',
  accountName: null,
  available: [],
  unavailable: [],
  summary: 'XING offers no sign-in for desktop apps and no API access for ReMa.',
  grantedScopes: [],
  grantAvailable: false,
};

const result: NetworkResult = {
  query: 'Find companies in Vienna hiring AI Engineers and the relevant recruiting leaders.',
  criteria: {
    locations: ['Vienna, Austria'],
    industries: [],
    companySize: null,
    technologies: [],
    roles: ['AI Engineer'],
    seniority: null,
    minOpenings: null,
    postedWithinDays: null,
    people: ['Recruiter'],
    targetCompany: null,
    relationships: true,
    usesProfile: false,
    limit: 20,
    stages: ['companies', 'jobs', 'people', 'connections'],
  },
  status: 'partial',
  companies: [
    {
      id: 'co_1',
      name: 'Donau Data',
      aliases: [],
      website: 'https://donau.example/',
      domain: 'donau.example',
      industry: null,
      size: null,
      employees: null,
      locations: ['Vienna, Austria'],
      linkedinUrl: 'https://www.linkedin.com/company/donau-data',
      xingUrl: null,
      otherUrls: [],
      matchedBecause: ['1 relevant opening found in this search (Senior AI Engineer)'],
      unverified: [],
      relevantOpenings: 1,
      evidence: [],
      lastVerifiedAt: 1,
    },
  ],
  jobs: [
    {
      id: 'rj_1',
      title: 'Senior AI Engineer',
      companyId: 'co_1',
      companyName: 'Donau Data',
      location: 'Vienna, Austria',
      workMode: null,
      postedAt: null,
      url: 'https://www.arbeitnow.com/jobs/companies/x/ai-engineer-donau-1',
      source: 'Arbeitnow',
      status: 'Posting read',
      notes: [],
    },
  ],
  people: [
    {
      id: 'pe_1',
      name: 'Lukas Gruber',
      title: 'Talent Acquisition Partner',
      companyId: 'co_1',
      companyName: 'Donau Data',
      location: null,
      linkedinUrl: null,
      xingUrl: null,
      otherUrl: null,
      relevance: 'named_recruiter',
      relevanceReason: 'the Senior AI Engineer posting names them as its contact',
      jobId: 'rj_1',
      confidence: 'high',
      evidence: [
        {
          source: 'jobs_mcp',
          sourceName: 'Arbeitnow',
          url: 'https://www.arbeitnow.com/jobs/companies/x/ai-engineer-donau-1',
          title: 'Senior AI Engineer',
          supports: 'named_on_posting',
          excerpt: 'Your contact: Lukas Gruber, Talent Acquisition Partner',
          retrievedAt: 1_790_380_800_000,
          publishedAt: null,
          checked: true,
        },
      ],
      relationship: null,
      source: 'jobs_mcp',
      class: 'professional_profile',
      persistence: 'persistent_permitted',
      fetchedAt: 1,
      caveat: null,
    },
  ],
  connections: [],
  connectionsOutcome: {
    state: 'unavailable',
    reason:
      'LinkedIn is connected for identity, but ReMa does not currently have permission to read your connection list, so ReMa cannot tell whom you know.',
  },
  rows: [{ companyId: 'co_1', jobId: 'rj_1', personId: 'pe_1' }],
  stages: [
    {
      stage: 'jobs',
      summary: '1 relevant opening at 1 employer (ReMa Jobs)',
      sources: ['ReMa Jobs', 'Arbeitnow'],
      failed: ['The Muse: could not be searched this time'],
    },
  ],
  notes: [],
  retrievedAt: 1_790_380_800_000,
  policyVersion: '2026-09-27',
};

function renderPage() {
  return render(
    <NavigationContext value={{ view: { page: 'network' }, navigate: () => {} }}>
      <NetworkConnectPage />
    </NavigationContext>,
  );
}

describe('NetworkConnectPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.getNetworkCapabilities.mockResolvedValue([linkedin, xing]);
    mocks.getLastNetworkResult.mockResolvedValue(null);
  });

  it('shows what each network actually grants', async () => {
    renderPage();
    const card = await screen.findByRole('region', { name: 'LinkedIn' });
    expect(within(card).getByText('Connected as Ana Example')).toBeTruthy();
    expect(within(card).getByText('Identity')).toBeTruthy();
    expect(within(card).getByText('Not available to ReMa now')).toBeTruthy();
    expect(within(card).getByText('Connection-list access')).toBeTruthy();
    // What it would take is said, not left to a tooltip.
    expect(within(card).getByText('— LinkedIn has not granted ReMa access to connection lists.')).toBeTruthy();
    // Technical scope names only under advanced details.
    expect(within(card).queryByText(/openid/)).toBeNull();
    fireEvent.click(within(card).getByRole('button', { name: 'Advanced details' }));
    expect(within(card).getByText('openid profile email')).toBeTruthy();
    const xingCard = screen.getByRole('region', { name: 'XING' });
    expect(within(xingCard).getByText('Not available')).toBeTruthy();
    expect(within(xingCard).queryByRole('button', { name: 'Connect' })).toBeNull();
    // Nothing more to grant: no sign-in button on a connected card.
    expect(within(card).queryByRole('button', { name: 'Grant connection access' })).toBeNull();
  });

  it('offers to sign in again when the app may read connections but the sign-in did not grant it', async () => {
    mocks.getNetworkCapabilities.mockResolvedValue([{ ...linkedin, grantAvailable: true }, xing]);
    mocks.connectConnector.mockResolvedValue(null);
    renderPage();
    const card = await screen.findByRole('region', { name: 'LinkedIn' });
    fireEvent.click(within(card).getByRole('button', { name: 'Grant connection access' }));
    await waitFor(() => expect(mocks.connectConnector).toHaveBeenCalledWith('linkedin'));
  });

  it('runs a request and shows the unified table, never "no connections"', async () => {
    mocks.researchNetwork.mockResolvedValue(result);
    renderPage();
    const box = await screen.findByLabelText('Research request');
    fireEvent.change(box, { target: { value: result.query } });
    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    await screen.findByText('Lukas Gruber');
    const call = mocks.researchNetwork.mock.calls[0]?.[0] as
      | { query: string; runId: string }
      | undefined;
    expect(call?.query).toBe(result.query);
    expect(call?.runId).toMatch(/^nc-/);
    expect(screen.getByText('Named contact on the posting')).toBeTruthy();
    expect(screen.getByText('High')).toBeTruthy();
    expect(screen.getByText('Partial: some sources could not be searched')).toBeTruthy();
    expect(screen.getByText(/cannot tell whom you know/)).toBeTruthy();
    expect(screen.queryByText(/no connections/i)).toBeNull();

    // Links open in the system browser.
    fireEvent.click(screen.getByRole('button', { name: 'Website' }));
    await waitFor(() => expect(mocks.openExternalUrl).toHaveBeenCalledWith('https://donau.example/'));

    // The drill-down shows the evidence.
    fireEvent.click(screen.getByRole('button', { name: 'Donau Data' }));
    const dialog = await screen.findByRole('dialog', { name: 'Lukas Gruber' });
    expect(within(dialog).getByText(/Your contact: Lukas Gruber/)).toBeTruthy();
    expect(within(dialog).getByText('Named on the posting')).toBeTruthy();
  });
});
