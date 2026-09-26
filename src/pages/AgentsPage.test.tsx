// @vitest-environment jsdom
import '../test/dom';

import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { NavigationContext, type View } from '../app/navigation';
import type { Agent } from '../services/agentService';
import { AgentsPage } from './AgentsPage';

const mocks = vi.hoisted(() => ({
  listAgents: vi.fn(),
  saveAgent: vi.fn(),
  duplicateAgent: vi.fn(),
  deleteAgent: vi.fn(),
}));

vi.mock('../services/agentService', async (original) => ({
  ...(await original<typeof import('../services/agentService')>()),
  ...mocks,
}));

const agent = (id: string, name: string, builtin: boolean): Agent => ({
  id,
  name,
  description: `${name} description`,
  instructions: `${name} instructions`,
  icon: 'spark',
  builtin,
  updatedAt: builtin ? null : 1_700_000_000_000,
});

const AGENTS = [
  agent('builtin:job-search', 'Job Search Agent', true),
  agent('builtin:job-match', 'Job Match Analyst', true),
  agent('custom:4', 'Cover Letter Writer', false),
];

function renderPage() {
  const navigate = vi.fn<(view: View) => void>();
  render(
    <NavigationContext value={{ view: { page: 'agents' }, navigate }}>
      <AgentsPage />
    </NavigationContext>,
  );
  return navigate;
}

beforeEach(() => {
  vi.clearAllMocks();
  mocks.listAgents.mockResolvedValue(AGENTS);
});

describe('Agents page', () => {
  it('lists built-in and custom agents', async () => {
    renderPage();
    const builtins = await screen.findByRole('region', { name: 'Built-in agents' });
    expect(within(builtins).getByText('Job Search Agent')).toBeTruthy();
    const mine = screen.getByRole('region', { name: 'My agents' });
    expect(within(mine).getByText('Cover Letter Writer')).toBeTruthy();
    expect(within(mine).getByText('Custom')).toBeTruthy();
  });

  it('creates a custom agent', async () => {
    mocks.saveAgent.mockResolvedValue(agent('custom:5', 'Salary Coach', false));
    renderPage();
    fireEvent.click(await screen.findByRole('button', { name: 'New agent' }));
    const dialog = await screen.findByRole('dialog', { name: 'New agent' });
    fireEvent.change(within(dialog).getByLabelText('Name'), { target: { value: 'Salary Coach' } });
    fireEvent.change(within(dialog).getByLabelText('Instructions'), { target: { value: 'Help negotiate offers.' } });
    fireEvent.click(within(dialog).getByRole('radio', { name: 'Target' }));
    fireEvent.click(within(dialog).getByRole('button', { name: 'Create agent' }));
    await waitFor(() =>
      expect(mocks.saveAgent).toHaveBeenCalledWith(null, {
        name: 'Salary Coach',
        description: '',
        instructions: 'Help negotiate offers.',
        icon: 'target',
      }),
    );
  });

  it('edits, renames and deletes custom agents only', async () => {
    mocks.saveAgent.mockResolvedValue(AGENTS[2]);
    mocks.deleteAgent.mockResolvedValue(null);
    renderPage();
    const card = await screen.findByRole('article', { name: 'Cover Letter Writer' });
    fireEvent.click(within(card).getByRole('button', { name: 'Edit' }));
    const dialog = await screen.findByRole('dialog', { name: 'Edit agent' });
    fireEvent.change(within(dialog).getByLabelText('Instructions'), { target: { value: 'Shorter letters.' } });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Save' }));
    await waitFor(() =>
      expect(mocks.saveAgent).toHaveBeenCalledWith(4, expect.objectContaining({ instructions: 'Shorter letters.' })),
    );

    fireEvent.click(within(card).getByRole('button', { name: 'More actions' }));
    fireEvent.click(screen.getByRole('menuitem', { name: 'Rename' }));
    const input = within(card).getByLabelText('Agent name');
    fireEvent.change(input, { target: { value: 'Letter Pro' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    await waitFor(() => expect(mocks.saveAgent).toHaveBeenLastCalledWith(4, expect.objectContaining({ name: 'Letter Pro' })));

    fireEvent.click(within(card).getByRole('button', { name: 'More actions' }));
    fireEvent.click(screen.getByRole('menuitem', { name: 'Delete' }));
    fireEvent.click(within(card).getByRole('button', { name: 'Delete' }));
    await waitFor(() => expect(mocks.deleteAgent).toHaveBeenCalledWith(4));

    // Built-in agents are stable: view or copy, never edit or delete.
    const builtin = screen.getByRole('article', { name: 'Job Search Agent' });
    fireEvent.click(within(builtin).getByRole('button', { name: 'More actions' }));
    const items = screen.getAllByRole('menuitem').map((i) => i.textContent);
    expect(items).toEqual(['View instructions', 'Customize a copy']);
  });

  it('customizing a built-in agent makes an editable copy', async () => {
    mocks.duplicateAgent.mockResolvedValue(agent('custom:6', 'Job Search Agent (copy)', false));
    renderPage();
    const card = await screen.findByRole('article', { name: 'Job Search Agent' });
    fireEvent.click(within(card).getByRole('button', { name: 'View' }));
    const dialog = await screen.findByRole('dialog', { name: 'Job Search Agent' });
    expect(within(dialog).getByText('Job Search Agent instructions')).toBeTruthy();
    fireEvent.click(within(dialog).getByRole('button', { name: 'Customize a copy' }));
    await waitFor(() => expect(mocks.duplicateAgent).toHaveBeenCalledWith('builtin:job-search'));
    expect(await screen.findByRole('dialog', { name: 'Edit agent' })).toBeTruthy();
  });

  it('"Use in chat" opens a new chat with the agent selected', async () => {
    const navigate = renderPage();
    const card = await screen.findByRole('article', { name: 'Job Match Analyst' });
    fireEvent.click(within(card).getByRole('button', { name: 'Use in chat' }));
    expect(navigate).toHaveBeenCalledWith({ page: 'chat', conversationId: null, agentIds: ['builtin:job-match'] });
  });
});
