// @vitest-environment jsdom
import '../../test/dom';

import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import type { ToolActivity } from '../../services/chatService';
import { WebActivity } from './WebActivity';

vi.mock('../../services/systemService', () => ({ openExternalUrl: vi.fn() }));

function activity(partial: Partial<ToolActivity>): ToolActivity {
  return {
    id: 'a',
    serverId: null,
    server: '',
    tool: '',
    status: 'completed',
    arguments: '',
    detail: null,
    readOnly: true,
    ...partial,
  };
}

const step = (status: ToolActivity['status'], text: string) =>
  activity({ id: 'web:retrieval', kind: 'retrieval', status, arguments: text });

describe('WebActivity: ReMa’s job search step', () => {
  it('shows live progress, then a one-line summary with the searches and checked pages', () => {
    const search = activity({
      id: 's1',
      kind: 'web_search',
      arguments: 'AI Engineer jobs Vienna',
      sources: [{ title: 'Senior AI Engineer', url: 'https://jobs.example.com/1' }],
    });
    const { rerender } = render(<WebActivity activity={[step('running', 'Checking 3 postings…'), search]} />);
    expect(screen.getByText('Checking 3 postings…')).toBeTruthy();

    rerender(
      <WebActivity
        activity={[
          step('completed', '1 search · 2 pages checked'),
          search,
          activity({ id: 'web:check:1', kind: 'web_page', tool: 'check', arguments: 'https://jobs.example.com/1' }),
          activity({
            id: 'web:check:2',
            kind: 'web_page',
            tool: 'check',
            status: 'failed',
            arguments: 'https://www.careers.example.org/2',
            detail: 'The posting is gone (404).',
          }),
        ]}
      />,
    );
    const summary = screen.getByRole('button', { name: /Searched the web/ });
    expect(summary.textContent).toContain('1 search · 2 pages checked');
    fireEvent.click(summary);
    expect(screen.getByText('AI Engineer jobs Vienna')).toBeTruthy();
    expect(screen.getByText(/^Checked/).textContent).toBe('Checked jobs.example.com');
    expect(screen.getByText(/^Could not check/).textContent).toBe('Could not check careers.example.org');
    expect(screen.getByText('The posting is gone (404).')).toBeTruthy();
  });

  it('says plainly when the search failed or was stopped', () => {
    const { rerender } = render(<WebActivity activity={[step('failed', 'No search service')]} />);
    expect(screen.getByText('Web search failed')).toBeTruthy();
    rerender(<WebActivity activity={[step('denied', '')]} />);
    expect(screen.getByText('Search stopped')).toBeTruthy();
  });
});
