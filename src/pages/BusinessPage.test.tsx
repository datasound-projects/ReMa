// @vitest-environment jsdom
import '../test/dom';

import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { NavigationContext, type BusinessTab, type View } from '../app/navigation';
import type {
  BusinessOverview,
  Claim,
  ClientProspect,
  ClientResults,
  ContractResults,
  Draft,
  Experiment,
  ExperimentMetrics,
  GtmPlan,
  Offer,
  OfferContent,
  Opportunity,
  Pipeline,
  Rate,
  VariantMetrics,
} from '../services/businessService';
import { BusinessPage } from './BusinessPage';

const mocks = vi.hoisted(() => ({
  getBusinessOverview: vi.fn(),
  getPipeline: vi.fn(),
  getLastBusinessResults: vi.fn(),
  findClients: vi.fn(),
  findContracts: vi.fn(),
  saveProspect: vi.fn(),
  saveContract: vi.fn(),
  createOffer: vi.fn(),
  saveOfferDraft: vi.fn(),
  reviewOffer: vi.fn(),
  changeStage: vi.fn(),
  recordActivity: vi.fn(),
  createDraft: vi.fn(),
  updateDraft: vi.fn(),
  getExperimentMetrics: vi.fn(),
  cancelBusinessRun: vi.fn(() => Promise.resolve(true)),
  openExternalUrl: vi.fn(() => Promise.resolve(null)),
}));

vi.mock('../services/businessService', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../services/businessService')>()),
  getBusinessOverview: mocks.getBusinessOverview,
  getPipeline: mocks.getPipeline,
  getLastBusinessResults: mocks.getLastBusinessResults,
  findClients: mocks.findClients,
  findContracts: mocks.findContracts,
  saveProspect: mocks.saveProspect,
  saveContract: mocks.saveContract,
  createOffer: mocks.createOffer,
  saveOfferDraft: mocks.saveOfferDraft,
  reviewOffer: mocks.reviewOffer,
  changeStage: mocks.changeStage,
  recordActivity: mocks.recordActivity,
  createDraft: mocks.createDraft,
  updateDraft: mocks.updateDraft,
  getExperimentMetrics: mocks.getExperimentMetrics,
  cancelBusinessRun: mocks.cancelBusinessRun,
}));
vi.mock('../services/systemService', () => ({
  openExternalUrl: mocks.openExternalUrl,
}));

const T = 1_790_380_800_000;

function claim(text: string, status: Claim['status'] = 'user_confirmed', sourceUrl: string | null = null): Claim {
  return { text, status, sourceUrl, retrievedAt: sourceUrl ? T : null, excerpt: sourceUrl ? text : null, note: null };
}

function content(patch: Partial<OfferContent> = {}): OfferContent {
  return {
    name: 'Support Workspace',
    kind: 'digital_product',
    maturity: 'pilot_ready',
    summary: claim('Shared inbox and knowledge search for support teams'),
    problem: claim('Support answers are scattered across tools'),
    outcomes: [],
    features: [claim('Salesforce integration', 'observed', 'https://acme.example/product')],
    useCases: [claim('Internal knowledge search')],
    customerTypes: [],
    buyerRoles: [claim('Head of Support')],
    pricing: [],
    deliveryModel: claim('SaaS'),
    geography: [],
    languages: [],
    requirements: [],
    integrations: [],
    deploymentConstraints: [],
    exclusions: [],
    unsupportedClaims: [],
    limitations: [],
    websiteUrls: ['https://acme.example/product'],
    ...patch,
  };
}

const offer: Offer = {
  id: 'off_1',
  name: 'Support Workspace',
  kind: 'digital_product',
  currentVersion: 2,
  draft: null,
  reviewed: content(),
  reviewedAt: T,
  archived: false,
  createdAt: T,
  updatedAt: T,
  revision: 3,
};

const ref = { offerId: 'off_1', version: 2, name: 'Support Workspace' };

function prospect(key: string, name: string, score: number | null, coverage: number, shown: boolean): ClientProspect {
  return {
    companyKey: key,
    companyName: name,
    website: `https://${key}.example/`,
    locations: ['Linz, Austria'],
    industry: 'Manufacturing',
    size: '51–200 employees',
    whyItFits: ['Internal knowledge search for service teams'],
    observedSignals: ['Hiring a support team lead (posting read)'],
    verifiedBuyingIntent: 'Not established: no source shows an active purchase.',
    permissionToContact: 'Not established: public visibility is not consent.',
    contacts: [
      {
        name: null,
        title: null,
        role: 'Head of Support',
        authority: 'unknown_authority',
        reason: 'The use case sits with support operations.',
        profileUrl: null,
        contactPage: `https://${key}.example/contact`,
        sourceUrl: null,
        suppressed: false,
      },
    ],
    assessment: {
      id: `fa_${key}`,
      companyKey: key,
      companyName: name,
      offer: ref,
      policyVersion: 'business-fit-2026-09-27',
      hard: [{ name: 'Location', result: 'pass', detail: 'Linz is in Austria.' }],
      criteria: [
        { id: 'use_case', label: 'Use case', weight: 0.35, applicable: true, value: 0.8, reason: 'Service teams.', evidence: [0] },
        { id: 'scale', label: 'Commercial scale', weight: 0.2, applicable: true, value: null, reason: 'Unknown.', evidence: [] },
      ],
      coverage,
      score,
      scoreShown: shown,
      evidence: [
        {
          label: 'Company website',
          url: `https://${key}.example/`,
          excerpt: 'Our service team supports 2,000 machines.',
          retrievedAt: T,
          contrary: false,
        },
      ],
      createdAt: T,
    },
    missing: shown ? [] : ['Company size'],
    suppressed: false,
    opportunityId: null,
    links: [],
  };
}

const clientResults: ClientResults = {
  runId: 'run-clients',
  status: 'complete',
  offer: ref,
  criteria: {
    locations: { countries: ['Austria'], regions: [], cities: [], radiusKm: null },
    industries: ['Manufacturing'],
    minEmployees: null,
    maxEmployees: null,
    exclusions: [],
    limit: 20,
  },
  icp: ['Manufacturers with service teams'],
  confirmed: [prospect('huber', 'Huber Maschinenbau', 72.4, 0.8, true)],
  needsVerification: [prospect('stahl', 'Stahl Werke', null, 0.4, false)],
  excluded: [],
  notes: [],
  sources: ['Wikidata', 'Company websites'],
  failures: [],
  retrievedAt: T,
  policyVersion: '2026-09-27',
  scoringPolicy: 'business-fit-2026-09-27',
};

const contractResults: ContractResults = {
  runId: 'run-contracts',
  status: 'complete',
  criteria: {
    skills: 'Python / AI',
    locations: { countries: ['Germany', 'Austria', 'Switzerland'], regions: [], cities: [], radiusKm: null },
    remoteOk: true,
    minRate: 700,
    rateComparison: 'above',
    currency: 'EUR',
    rateUnit: 'day',
    hoursPerDay: null,
    durationMinMonths: 1,
    durationMaxMonths: 6,
    postedWithinDays: null,
  },
  normalized: ['Germany, Austria or Switzerland (DACH)', 'Rate above EUR 700 per day', '1–6 months'],
  confirmed: [
    {
      key: 'c1',
      title: 'Python ML Engineer (freelance)',
      company: 'Orbit Consulting',
      location: 'Munich, Germany',
      url: 'https://jobs.example/c1',
      source: 'Arbeitnow',
      postedAt: T,
      verified: 'Posting read',
      terms: {
        engagement: 'freelance',
        rateMin: 800,
        rateMax: 900,
        rateIsCeiling: false,
        currency: 'EUR',
        rateUnit: 'day',
        rateText: null,
        durationMin: 3,
        durationMax: 6,
        durationUnit: 'month',
        durationText: null,
        extensionPossible: true,
        start: null,
        workload: null,
        workMode: 'Hybrid',
        eligibility: [],
        agency: 'Orbit Consulting',
        endClient: null,
        skills: ['Python', 'PyTorch'],
        languages: [],
      },
      status: 'confirmed',
      reasons: ['Daily rate from EUR 800 is above EUR 700.'],
      scope: 'Build a forecasting model.',
      opportunityId: null,
    },
  ],
  needsVerification: [],
  notMatching: [],
  notes: [],
  sources: ['ReMa Jobs'],
  failures: [],
  retrievedAt: T,
};

function rate(n: number, d: number): Rate {
  return d === 0
    ? { numerator: n, denominator: d, value: null, display: 'Not available' }
    : { numerator: n, denominator: d, value: n / d, display: `${n} / ${d} (${Math.round((n / d) * 100)}%)` };
}

function variantMetrics(variant: string | null, contacted: number, replied: number): VariantMetrics {
  return {
    variant,
    accountsInCohort: 5,
    accountsContacted: contacted,
    accountsReplied: replied,
    accountsPositiveReply: 0,
    accountsMet: 0,
    accountsProposed: 0,
    accountsWon: 0,
    replyRate: rate(replied, contacted),
    positiveReplyRate: rate(0, contacted),
    meetingRate: rate(0, contacted),
    proposalRate: rate(0, contacted),
    winRate: rate(0, contacted),
    peopleContacted: contacted,
  };
}

const opportunity: Opportunity = {
  id: 'opp_1',
  kind: 'product',
  name: 'Support Workspace for Huber',
  companyKey: 'huber',
  companyName: 'Huber Maschinenbau',
  offer: ref,
  canonicalJobId: null,
  sourceUrl: null,
  useCase: 'Internal knowledge search',
  stage: 'new_lead',
  archived: false,
  doNotContact: false,
  contacts: [{ id: 'role:head of support', name: null, title: null, role: 'Head of Support', profileUrl: null, sourceUrl: null }],
  evidence: [],
  amount: null,
  contract: null,
  listingStatus: null,
  nextStep: 'Ask about their service knowledge base',
  notes: '',
  createdAt: T,
  updatedAt: T,
  lastResearchedAt: T,
  lastCommercialActivityAt: null,
  revision: 4,
  assessment: null,
  assessments: 1,
  activities: [],
};

const draft: Draft = {
  id: 'dr_1',
  opportunityId: 'opp_1',
  planId: null,
  experimentId: null,
  variant: null,
  offer: ref,
  recipient: 'Head of Support',
  channel: 'Email',
  subject: 'A question about your service knowledge',
  body: 'Hello Head of Support team,\n\nhow do your technicians find answers today?',
  evidence: [],
  createdAt: T,
  updatedAt: T,
  revision: 0,
};

const plan: GtmPlan = {
  id: 'plan_1',
  offer: ref,
  name: 'Austria first',
  geography: 'Austria',
  content: {
    segments: [],
    alternatives: [],
    positioning: '',
    untestedClaims: [],
    channels: [],
    targetAccounts: [],
    notes: [],
  },
  createdAt: T,
  updatedAt: T,
  revision: 1,
};

const experiment: Experiment = {
  id: 'exp_1',
  planId: 'plan_1',
  offer: ref,
  segmentId: null,
  version: 1,
  status: 'running',
  content: {
    hypothesis: 'Support leads answer a question about downtime',
    channel: 'Email',
    variants: [
      { id: 'v1', label: 'A', message: 'Downtime question' },
      { id: 'v2', label: 'B', message: 'Feature list' },
    ],
    assignment: 'random_by_account',
    cohort: [],
    primaryMetric: 'reply_rate',
    successThreshold: null,
    plannedStart: T,
    plannedEnd: null,
    observationDays: 21,
    sampleTarget: 30,
    budget: null,
    effortBudget: null,
    stopConditions: [],
    exclusions: [],
    outcomeSummary: null,
    limitations: [],
  },
  frozenAt: T,
  createdAt: T,
  updatedAt: T,
  revision: 2,
};

const metrics: ExperimentMetrics = {
  experimentId: 'exp_1',
  windowStart: T,
  windowEnd: T + 21 * 86_400_000,
  overall: variantMetrics(null, 5, 2),
  variants: [variantMetrics('v1', 3, 2), variantMetrics('v2', 2, 0)],
  limitations: ['Activity is user-reported; ReMa does not verify it independently.'],
  unattributed: [],
  computedAt: T,
};

function overviewWith(patch: Partial<BusinessOverview> = {}): BusinessOverview {
  return {
    profile: {
      businessName: '',
      website: '',
      serviceArea: '',
      languages: [],
      capacity: '',
      availability: '',
      constraints: '',
      updatedAt: 0,
    },
    offers: [offer],
    plans: [],
    experiments: [],
    drafts: [],
    ...patch,
  };
}

const pipeline: Pipeline = {
  opportunities: [opportunity],
  counts: [
    ['new_lead', 1],
    ['qualified', 0],
    ['contacted', 0],
    ['discussion', 0],
    ['proposal', 0],
    ['won', 0],
    ['lost', 0],
  ],
  suppressions: [],
};

function renderPage(tab: BusinessTab, opportunityId: string | null = null) {
  let view: View = { page: 'business', tab, opportunityId };
  const navigate = vi.fn((next: View) => {
    view = next;
  });
  const result = render(
    <NavigationContext value={{ view, navigate }}>
      <BusinessPage tab={tab} opportunityId={opportunityId} />
    </NavigationContext>,
  );
  return { ...result, navigate };
}

describe('BusinessPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    try {
      window.localStorage.clear();
    } catch {
      // ignore
    }
    mocks.getBusinessOverview.mockResolvedValue(overviewWith());
    mocks.getPipeline.mockResolvedValue(pipeline);
    mocks.getLastBusinessResults.mockResolvedValue({ clients: null, contracts: null });
  });

  it('shows the four views and keeps research off until an offer is reviewed', async () => {
    mocks.getBusinessOverview.mockResolvedValue(overviewWith({ offers: [] }));
    renderPage('clients');
    const tabs = await screen.findByRole('tablist', { name: 'Business' });
    expect(within(tabs).getAllByRole('tab').map((t) => t.textContent)).toEqual([
      'Find Clients',
      'Find Contract Work',
      'Go-to-Market Studio',
      'Pipeline',
    ]);
    expect(screen.getByRole('button', { name: 'Business Profile' })).toBeTruthy();
    expect(screen.getByText(/Describe what you offer first/)).toBeTruthy();
    expect((screen.getByRole('button', { name: 'Find clients' }) as HTMLButtonElement).disabled).toBe(true);
  });

  it('finds clients for the reviewed offer and saves one as a New Lead', async () => {
    mocks.findClients.mockResolvedValue(clientResults);
    mocks.saveProspect.mockResolvedValue({ opportunity, created: true });
    renderPage('clients');
    const box = await screen.findByLabelText('Which clients to look for');
    fireEvent.change(box, { target: { value: 'Find Austrian manufacturers' } });
    fireEvent.click(screen.getByRole('button', { name: 'Find clients' }));
    await screen.findByText('Huber Maschinenbau');
    const input = mocks.findClients.mock.calls[0]?.[0] as { offerId: string; offerVersion: number; runId: string } | undefined;
    expect(input?.offerId).toBe('off_1');
    expect(input?.offerVersion).toBe(2);
    expect(input?.runId).toMatch(/^run-/);

    // Fit is an evidence measure with its coverage, never a chance of buying.
    expect(screen.getByText('72 (evidence coverage 80%)')).toBeTruthy();
    expect(screen.getByText('Insufficient evidence (coverage 40%)')).toBeTruthy();
    expect(screen.queryByText(/likely to buy/i)).toBeNull();
    expect(screen.getByText('Confirmed matches')).toBeTruthy();
    expect(screen.getByText('Needs verification')).toBeTruthy();
    expect(screen.getByText(/a hypothesis, not confirmed demand/)).toBeTruthy();

    fireEvent.click(screen.getByRole('button', { name: 'Huber Maschinenbau' }));
    expect(await screen.findByText(/Not established: no source shows an active purchase/)).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Save to Pipeline' }));
    await screen.findByText('Saved as a New Lead.');
    expect(mocks.saveProspect).toHaveBeenCalledWith('run-clients', 'huber', 'Internal knowledge search for service teams');
  });

  it('shows contract terms as stated and keeps the agency apart from the client', async () => {
    mocks.findContracts.mockResolvedValue(contractResults);
    mocks.saveContract.mockResolvedValue({ opportunity: { ...opportunity, kind: 'contract' }, created: true });
    renderPage('contracts');
    const box = await screen.findByLabelText('Which contract work to look for');
    fireEvent.change(box, { target: { value: 'Python/AI contracts in DACH lasting 1–6 months above EUR 700/day' } });
    fireEvent.click(screen.getByRole('button', { name: 'Find contract work' }));
    await screen.findByText('Python ML Engineer (freelance)');
    expect(mocks.findContracts.mock.calls[0]?.[0]).toMatchObject({ criteria: null });
    expect(screen.getByText('Rate above EUR 700 per day')).toBeTruthy();
    expect(screen.getByText('EUR 800–900 per day')).toBeTruthy();
    expect(screen.getByText('End client not disclosed')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Python ML Engineer (freelance)' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Save to Pipeline' }));
    await screen.findByText(/no Application was created/);
    expect(mocks.saveContract).toHaveBeenCalledWith('run-contracts', 'c1');
  });

  it('moves a stage only with the activity it rests on', async () => {
    mocks.changeStage.mockResolvedValue({ ...opportunity, stage: 'contacted' });
    renderPage('pipeline', 'opp_1');
    const dialog = await screen.findByRole('dialog', { name: 'Support Workspace for Huber' });
    fireEvent.click(within(dialog).getByRole('tab', { name: /Stage & activity/ }));
    fireEvent.change(within(dialog).getByLabelText('Move to'), { target: { value: 'contacted' } });
    expect(within(dialog).getByText(/a draft or an opened link does not count/)).toBeTruthy();
    fireEvent.click(within(dialog).getByRole('button', { name: 'Move to Contacted' }));
    await waitFor(() => expect(mocks.changeStage).toHaveBeenCalled());
    const [id, change] = mocks.changeStage.mock.calls[0] as [string, { to: string; expectedRevision: number; occurredAt: number | null; idempotencyKey: string }];
    expect(id).toBe('opp_1');
    expect(change.to).toBe('contacted');
    expect(change.expectedRevision).toBe(4);
    expect(change.occurredAt).not.toBeNull();
    expect(change.idempotencyKey).toMatch(/^stage-/);

    // Won is an accepted engagement, not money received.
    fireEvent.change(within(dialog).getByLabelText('Move to'), { target: { value: 'won' } });
    expect(within(dialog).getByText(/An engagement or order was accepted \(not money received\)/)).toBeTruthy();
  });

  it('keeps drafts local: copying records nothing and there is no send action', async () => {
    const writeText = vi.fn(() => Promise.resolve());
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
    mocks.getBusinessOverview.mockResolvedValue(overviewWith({ drafts: [draft] }));
    renderPage('pipeline', 'opp_1');
    const dialog = await screen.findByRole('dialog', { name: 'Support Workspace for Huber' });
    fireEvent.click(within(dialog).getByRole('tab', { name: /Drafts/ }));
    expect(within(dialog).getByText('Local draft — not sent')).toBeTruthy();
    fireEvent.click(within(dialog).getByRole('button', { name: 'Copy draft' }));
    await within(dialog).findByText(/Copying is not contact/);
    expect(writeText).toHaveBeenCalled();
    expect(mocks.recordActivity).not.toHaveBeenCalled();
    expect(mocks.changeStage).not.toHaveBeenCalled();
    expect(screen.queryByRole('button', { name: /send/i })).toBeNull();
  });

  it('shows experiment results as numerator / denominator, user-reported', async () => {
    mocks.getBusinessOverview.mockResolvedValue(overviewWith({ plans: [plan], experiments: [experiment] }));
    mocks.getExperimentMetrics.mockResolvedValue(metrics);
    renderPage('gtm');
    fireEvent.click(await screen.findByRole('tab', { name: 'Experiments' }));
    await screen.findByText('2 / 5 (40%)');
    expect(screen.getByText('2 / 3 (67%)')).toBeTruthy();
    expect(screen.getByText(/user-reported activity/)).toBeTruthy();
    expect(screen.getByText(/does not verify it independently/)).toBeTruthy();
  });

  it('reviews an offer: confirming claims and saving a version', async () => {
    const withDraft: Offer = {
      ...offer,
      currentVersion: null,
      reviewed: null,
      draft: content({ problem: claim('', 'unknown') }),
    };
    mocks.getBusinessOverview.mockResolvedValue(overviewWith({ offers: [withDraft] }));
    mocks.saveOfferDraft.mockImplementation((_id: string, c: OfferContent, rev: number) =>
      Promise.resolve({ ...withDraft, draft: c, revision: rev + 1 }),
    );
    mocks.reviewOffer.mockResolvedValue({ ...withDraft, currentVersion: 1, revision: 5 });
    renderPage('clients');
    fireEvent.click(await screen.findByRole('button', { name: 'Open offer' }));
    await screen.findByText('Not reviewed yet: research cannot use it until you save a reviewed version.');
    // The website's claim is marked as observed, not confirmed.
    expect(screen.getByText('From the source — not confirmed')).toBeTruthy();
    const save = screen.getByRole('button', { name: 'Save reviewed version' }) as HTMLButtonElement;
    expect(save.disabled).toBe(true);
    expect(screen.getByText('Describe the problem it addresses.')).toBeTruthy();
    fireEvent.change(screen.getByLabelText('Problem it addresses'), { target: { value: 'Answers are scattered' } });
    fireEvent.click(screen.getByRole('button', { name: 'Confirm' }));
    expect(save.disabled).toBe(false);
    fireEvent.click(save);
    await screen.findByText(/Saved as reviewed version 1/);
    const saved = mocks.saveOfferDraft.mock.calls[0]?.[1] as OfferContent | undefined;
    expect(saved?.problem).toMatchObject({ text: 'Answers are scattered', status: 'user_confirmed' });
    expect(saved?.features[0]?.status).toBe('user_confirmed');
    expect(mocks.reviewOffer).toHaveBeenCalledWith('off_1', 4);
  });
});
