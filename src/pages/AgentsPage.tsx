import { useState } from 'react';

import { useNavigation } from '../app/navigation';
import { AgentDialog } from '../components/agents/AgentDialog';
import { AgentIcon } from '../components/agents/AgentIcon';
import { AgentIcon as AgentsIcon, MoreIcon, PlusIcon } from '../components/icons';
import { PageContainer } from '../components/layout/PageContainer';
import { EmptyState, LoadingState } from '../components/ui/EmptyState';
import { Menu, type MenuItem } from '../components/ui/Menu';
import { useAction } from '../hooks/useAction';
import { useAgents } from '../hooks/useAgents';
import { formatDateTime } from '../lib/format';
import { customAgentNumber, deleteAgent, duplicateAgent, saveAgent, type Agent } from '../services/agentService';

/**
 * Reusable instructions that shape how the model works in Chat. Built-in
 * agents are fixed; the user's own are created, edited and removed here,
 * and selected per chat with the composer's + button.
 */
export function AgentsPage() {
  const loaded = useAgents();
  const { navigate } = useNavigation();
  const [editing, setEditing] = useState<Agent | 'new' | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const action = useAction();

  const agents = loaded.state.status === 'success' ? loaded.state.data : [];
  const builtins = agents.filter((a) => a.builtin);
  const mine = agents.filter((a) => !a.builtin);

  const duplicate = (agent: Agent) =>
    void action.run(async () => {
      const copy = await duplicateAgent(agent.id);
      setEditing(copy);
      setNotice(`Created “${copy.name}”. Edit it to make it yours.`);
    });

  const openInChat = (agent: Agent) => navigate({ page: 'chat', conversationId: null, agentIds: [agent.id] });

  return (
    <PageContainer
      title="Agents"
      subtitle="Reusable instructions for the model. Pick one or more with + in the chat composer."
      actions={
        <button type="button" className="button button--primary" onClick={() => setEditing('new')}>
          <PlusIcon className="button__icon" />
          New agent
        </button>
      }
    >
      {loaded.state.status === 'loading' && <LoadingState label="Loading agents…" />}
      {loaded.state.status === 'error' && (
        <div className="notice notice--danger" role="alert">
          {loaded.state.error.message}{' '}
          <button type="button" className="link-button" onClick={loaded.retry}>
            Try again
          </button>
        </div>
      )}
      {(notice || action.error) && (
        <p className={action.error ? 'notice notice--danger' : 'notice'} role="status">
          {action.error ?? notice}
        </p>
      )}

      {loaded.state.status === 'success' && (
        <>
          <section className="section" aria-labelledby="agents-mine">
            <div className="section__head">
              <div className="section__heading">
                <h2 className="section__title" id="agents-mine">
                  My agents
                </h2>
                <p className="section__description">Your own agents, stored on this computer.</p>
              </div>
            </div>
            {mine.length === 0 ? (
              <EmptyState
                framed
                icon={<AgentsIcon />}
                title="No agents of your own yet"
                actions={
                  <button type="button" className="button button--secondary" onClick={() => setEditing('new')}>
                    <PlusIcon className="button__icon" />
                    New agent
                  </button>
                }
              >
                Write instructions once, reuse them in any chat. You can also customize a copy of a built-in agent.
              </EmptyState>
            ) : (
              <div className="agent-grid">
                {mine.map((agent) => (
                  <AgentCard
                    key={agent.id}
                    agent={agent}
                    onOpen={() => setEditing(agent)}
                    onUse={() => openInChat(agent)}
                    onDuplicate={() => duplicate(agent)}
                  />
                ))}
              </div>
            )}
          </section>

          <section className="section" aria-labelledby="agents-builtin">
            <div className="section__head">
              <div className="section__heading">
                <h2 className="section__title" id="agents-builtin">
                  Built-in agents
                </h2>
                <p className="section__description">Made by ReMa. To change one, customize a copy.</p>
              </div>
            </div>
            <div className="agent-grid">
              {builtins.map((agent) => (
                <AgentCard
                  key={agent.id}
                  agent={agent}
                  onOpen={() => setEditing(agent)}
                  onUse={() => openInChat(agent)}
                  onDuplicate={() => duplicate(agent)}
                />
              ))}
            </div>
          </section>
        </>
      )}

      {editing !== null && (
        <AgentDialog
          key={editing === 'new' ? 'new' : editing.id}
          agent={editing === 'new' ? null : editing}
          onClose={() => {
            setEditing(null);
            setNotice(null);
          }}
          onDuplicate={(agent) => duplicate(agent)}
        />
      )}
    </PageContainer>
  );
}

function AgentCard({
  agent,
  onOpen,
  onUse,
  onDuplicate,
}: {
  agent: Agent;
  onOpen: () => void;
  onUse: () => void;
  onDuplicate: () => void;
}) {
  const action = useAction();
  const [name, setName] = useState<string | null>(null);
  const [confirming, setConfirming] = useState(false);
  const number = customAgentNumber(agent);

  const rename = () => {
    const next = name?.trim();
    setName(null);
    if (next && next !== agent.name && number !== null) {
      void action.run(() =>
        saveAgent(number, { name: next, description: agent.description, instructions: agent.instructions, icon: agent.icon }),
      );
    }
  };

  const items: MenuItem[] = agent.builtin
    ? [
        { label: 'View instructions', onSelect: onOpen },
        { label: 'Customize a copy', onSelect: onDuplicate },
      ]
    : [
        { label: 'Edit', onSelect: onOpen },
        { label: 'Rename', onSelect: () => setName(agent.name) },
        { label: 'Duplicate', onSelect: onDuplicate },
        { label: 'Delete', danger: true, onSelect: () => setConfirming(true) },
      ];

  return (
    <article className="agent-card" aria-label={agent.name}>
      <div className="agent-card__head">
        <span className="agent-card__icon" aria-hidden="true">
          <AgentIcon icon={agent.icon} />
        </span>
        <div className="agent-card__title">
          {name === null ? (
            <button type="button" className="agent-card__name" onClick={onOpen}>
              {agent.name}
            </button>
          ) : (
            <input
              className="input input--small"
              aria-label="Agent name"
              autoFocus
              maxLength={80}
              value={name}
              onChange={(e) => setName(e.target.value)}
              onBlur={rename}
              onKeyDown={(e) => {
                if (e.key === 'Enter') rename();
                if (e.key === 'Escape') setName(null);
              }}
            />
          )}
          <span className={agent.builtin ? 'badge' : 'badge badge--brand'}>{agent.builtin ? 'Built-in' : 'Custom'}</span>
        </div>
        <Menu
          items={items}
          trigger={(props) => (
            <button type="button" className="icon-button icon-button--small" aria-label="More actions" title="More actions" {...props}>
              <MoreIcon />
            </button>
          )}
        />
      </div>
      <p className="agent-card__description">{agent.description || 'No description.'}</p>
      {agent.updatedAt !== null && <span className="agent-card__meta">Edited {formatDateTime(agent.updatedAt)}</span>}
      {confirming ? (
        <div className="doc-card__confirm" role="group" aria-label="Confirm deletion">
          <span>Delete this agent? Chats that use it stop using it.</span>
          <button
            type="button"
            className="button button--danger button--small"
            disabled={action.busy}
            onClick={() => number !== null && void action.run(() => deleteAgent(number))}
          >
            Delete
          </button>
          <button type="button" className="button button--ghost button--small" onClick={() => setConfirming(false)}>
            Cancel
          </button>
        </div>
      ) : (
        <div className="agent-card__foot">
          <span className="agent-card__spacer" />
          <button type="button" className="button button--ghost button--small" onClick={onOpen}>
            {agent.builtin ? 'View' : 'Edit'}
          </button>
          <button type="button" className="button button--secondary button--small" onClick={onUse}>
            Use in chat
          </button>
        </div>
      )}
      {action.error && <p className="form-error">{action.error}</p>}
    </article>
  );
}
