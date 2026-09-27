import { describe, expect, it } from 'vitest';

import { describeWebSearch, elapsed, failureGuidance, runStatus, runTags, runTitle } from './taskRuns';

const noon = new Date(2026, 8, 27, 12, 0);

describe('run history labels', () => {
  it('names runs by when they happened', () => {
    expect(runTitle({ queuedAt: new Date(2026, 8, 27, 9, 30).getTime() }, noon)).toMatch(/^Run — Today at /);
    expect(runTitle({ queuedAt: new Date(2026, 8, 26, 21, 30).getTime() }, noon)).toMatch(/^Run — Yesterday at /);
    expect(runTitle({ queuedAt: new Date(2026, 8, 24, 21, 30).getTime() }, noon)).toMatch(/^Run — 24 Sep at |^Run — Sep 24 at /);
  });

  it('tells manual, failed and skipped runs apart, and leaves successes quiet', () => {
    expect(runTags({ trigger: 'scheduled', status: 'succeeded', errorCategory: null })).toEqual([]);
    expect(runTags({ trigger: 'manual', status: 'succeeded', errorCategory: null })).toEqual(['Manual']);
    expect(runTags({ trigger: 'scheduled', status: 'failed', errorCategory: 'provider' })).toEqual(['Failed']);
    expect(runTags({ trigger: 'manual', status: 'running', errorCategory: null })).toEqual(['Manual', 'Running']);
    expect(runStatus({ status: 'cancelled', errorCategory: 'skipped' }).label).toBe('Skipped');
    expect(runStatus({ status: 'cancelled', errorCategory: 'cancelled' }).label).toBe('Cancelled');
    expect(runStatus({ status: 'queued', errorCategory: null })).toEqual({ tone: 'pending', label: 'Queued' });
  });

  it('says what to do about a failure', () => {
    expect(failureGuidance('connector')).toMatchObject({ action: 'connectors' });
    expect(failureGuidance('model_access')).toMatchObject({ action: 'settings' });
    expect(failureGuidance('task')).toMatchObject({ action: 'edit' });
    expect(failureGuidance('interrupted')?.text).toMatch(/closed or stopped/);
    expect(failureGuidance(null)).toBeNull();
  });

  it('describes the web search a run had', () => {
    const base = { profile: false, connectors: [], webSearch: true, searches: 0, searchEngines: [] };
    expect(describeWebSearch({ ...base, webSearch: false })).toBe('Not available');
    expect(describeWebSearch(base)).toBe('Enabled · not used');
    expect(describeWebSearch({ ...base, searches: 1, searchEngines: ['Anthropic web search'] })).toBe(
      'Enabled · Anthropic web search · 1 search',
    );
  });

  it('measures a run while it runs and after it ended', () => {
    expect(elapsed({ startedAt: null, finishedAt: null }, 10)).toBeNull();
    expect(elapsed({ startedAt: 1_000, finishedAt: null }, 19_000)).toBe(18_000);
    expect(elapsed({ startedAt: 1_000, finishedAt: 49_000 }, 99_000)).toBe(48_000);
  });
});
