// @vitest-environment jsdom
import '../../test/dom';

import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { CareerSearchStatus } from '../../services/careerSearchService';
import { CareerSearchSection } from './CareerSearchSection';

const mocks = vi.hoisted(() => ({
  careerSearchStatus: vi.fn(),
  checkCareerSearch: vi.fn(),
  webSearchSettings: vi.fn(),
  saveWebSearchSettings: vi.fn(),
  testWebSearch: vi.fn(),
}));

vi.mock('../../services/careerSearchService', () => ({
  careerSearchStatus: mocks.careerSearchStatus,
  checkCareerSearch: mocks.checkCareerSearch,
}));
vi.mock('../../services/webSearchService', () => ({
  webSearchSettings: mocks.webSearchSettings,
  saveWebSearchSettings: mocks.saveWebSearchSettings,
  testWebSearch: mocks.testWebSearch,
}));

const status: CareerSearchStatus = {
  automatic: true,
  model: 'Qwen 3 (local)',
  modelSearch: null,
  capabilities: {
    provider: 'OpenAI-Compatible',
    authMode: 'local_server',
    runtime: 'openai_compatible',
    inferenceAvailable: true,
    nativeSearchAvailable: false,
    nativeSearchLive: false,
    nativeDomainFiltering: false,
    nativeCitations: false,
    remaSearchAvailable: true,
    nativeDetail: null,
    note: 'no web search of its own; ReMa searches for it',
  },
  routes: [
    { name: 'ReMa job sources', detail: 'Employers’ own job boards and public job boards. No key needed.', available: true },
    { name: 'Company and people research', detail: 'Wikidata, Wikipedia and company websites.', available: true },
    { name: 'Model web search', detail: 'This model has no web search of its own; ReMa searches for it.', available: false },
  ],
  extraService: null,
  sources: [],
  recent: [
    {
      at: 1_790_380_800_000,
      requirement: 'required',
      scopes: ['Jobs'],
      route: 'ReMa Jobs',
      fallbacks: [],
      queried: ['ReMa Jobs'],
      succeeded: ['ReMa Jobs'],
      failed: [],
      durationMs: 2_400,
      results: 7,
    },
  ],
  checked: [],
};

beforeEach(() => {
  vi.clearAllMocks();
  mocks.careerSearchStatus.mockResolvedValue(status);
  mocks.webSearchSettings.mockResolvedValue({ service: null, url: null, hasKey: false });
});

describe('Settings → Career Search', () => {
  it('is automatic, needs no key and lists its routes', async () => {
    render(<CareerSearchSection />);
    expect(screen.getByText('Automatic')).toBeTruthy();
    expect(screen.getByText(/No API key required\./)).toBeTruthy();
    const routes = await screen.findByRole('list', { name: 'Search routes' });
    expect(within(routes).getByText('ReMa job sources')).toBeTruthy();
    expect(within(routes).getByText(/ReMa searches for it/)).toBeTruthy();
    expect(screen.getByText(/Selected model:/).textContent).toContain('Qwen 3 (local) — ReMa searches for it');
    expect(screen.getByText('Local or compatible server · ReMa searches for it')).toBeTruthy();
    // Nothing asks the user to set up a search service.
    expect(document.body.textContent).not.toMatch(/configure|set up a search/i);
  });

  it('shows what the selected model’s runtime can do, not a setting', async () => {
    mocks.careerSearchStatus.mockResolvedValue({
      ...status,
      model: 'Claude Sonnet 5',
      modelSearch: 'Anthropic web search',
      capabilities: {
        ...status.capabilities!,
        provider: 'Anthropic',
        authMode: 'api_key',
        runtime: 'anthropic_messages',
        nativeSearchAvailable: true,
        nativeSearchLive: true,
        nativeDomainFiltering: true,
        nativeCitations: true,
        nativeDetail: 'web_search_20260318',
        note: null,
      },
    });
    render(<CareerSearchSection />);
    expect(
      await screen.findByText(
        'Anthropic Messages API · own web search (web_search_20260318) · live · kept to career sites · reports its sources',
      ),
    ).toBeTruthy();
    expect(screen.queryByRole('checkbox')).toBeNull();
  });

  it('says a ChatGPT account searches live, once', async () => {
    mocks.careerSearchStatus.mockResolvedValue({
      ...status,
      model: 'GPT-6-Astra',
      modelSearch: 'ChatGPT web search',
      capabilities: {
        ...status.capabilities!,
        provider: 'OpenAI',
        authMode: 'chatgpt_account',
        runtime: 'codex_app_server',
        nativeSearchAvailable: true,
        nativeSearchLive: true,
        nativeDomainFiltering: true,
        nativeCitations: false,
        nativeDetail: 'live',
        note: null,
      },
    });
    render(<CareerSearchSection />);
    expect(
      await screen.findByText('Codex (ChatGPT account) · own web search (live) · kept to career sites'),
    ).toBeTruthy();
  });

  it('checks the sources on demand and shows recent searches', async () => {
    mocks.checkCareerSearch.mockResolvedValue({
      ...status,
      checked: [
        { name: 'ReMa job sources', ok: true, detail: 'Answered (100 current jobs on the first page).' },
        { name: 'Company research', ok: false, detail: 'Not reachable right now: the source did not answer in time.' },
      ],
    });
    render(<CareerSearchSection />);
    fireEvent.click(await screen.findByRole('button', { name: 'Check now' }));
    expect(await screen.findByText('Answered (100 current jobs on the first page).')).toBeTruthy();
    expect(screen.getByText(/Not reachable right now/)).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: /Recent searches/ }));
    expect(screen.getByText(/ReMa Jobs · 7 results · 2\.4 s/)).toBeTruthy();
  });

  it('keeps an additional search service optional, under Advanced', async () => {
    render(<CareerSearchSection />);
    const advanced = screen.getByRole('button', { name: /Advanced \(optional\)/ });
    expect(advanced.getAttribute('aria-expanded')).toBe('false');
    fireEvent.click(advanced);
    const select = (await screen.findByLabelText('Search service')) as HTMLSelectElement;
    await waitFor(() => expect(select.disabled).toBe(false));
    expect(select.value).toBe('');
    expect(screen.getByRole('option', { name: 'None (not needed)' })).toBeTruthy();
    expect(screen.getByText(/Career search works the same without one\./)).toBeTruthy();
  });
});
