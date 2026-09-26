import { useState } from 'react';

import { toApiError } from '../../services/ipc';
import { customAgentNumber, saveAgent, type Agent, type AgentInput } from '../../services/agentService';
import { Dialog } from '../ui/Dialog';
import { FormField } from '../ui/FormField';
import { AGENT_ICONS } from './agentIcons';

interface AgentDialogProps {
  /** `null` creates a new agent. Built-in agents open read-only. */
  agent: Agent | null;
  onClose: () => void;
  /** Built-in agents: make an editable copy. */
  onDuplicate?: (agent: Agent) => void;
}

const empty: AgentInput = { name: '', description: '', instructions: '', icon: 'spark' };

/** Creates or edits a custom agent; shows a built-in agent's instructions. */
export function AgentDialog({ agent, onClose, onDuplicate }: AgentDialogProps) {
  const readOnly = agent?.builtin === true;
  const [form, setForm] = useState<AgentInput>(() =>
    agent ? { name: agent.name, description: agent.description, instructions: agent.instructions, icon: agent.icon } : empty,
  );
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const set = (patch: Partial<AgentInput>) => setForm((f) => ({ ...f, ...patch }));

  const save = async () => {
    setBusy(true);
    setError(null);
    try {
      await saveAgent(agent ? customAgentNumber(agent) : null, form);
      onClose();
    } catch (err) {
      setError(toApiError(err).message);
      setBusy(false);
    }
  };

  const title = readOnly ? agent.name : agent ? 'Edit agent' : 'New agent';

  return (
    <Dialog
      title={title}
      size="wide"
      onClose={onClose}
      actions={
        readOnly ? (
          <>
            <button type="button" className="button button--secondary" onClick={onClose}>
              Close
            </button>
            {onDuplicate && (
              <button type="button" className="button button--primary" onClick={() => onDuplicate(agent)}>
                Customize a copy
              </button>
            )}
          </>
        ) : (
          <>
            {error && <span className="form-error">{error}</span>}
            <button type="button" className="button button--secondary" onClick={onClose}>
              Cancel
            </button>
            <button
              type="button"
              className="button button--primary"
              disabled={busy || !form.name.trim() || !form.instructions.trim()}
              onClick={() => void save()}
            >
              {busy ? 'Saving…' : agent ? 'Save' : 'Create agent'}
            </button>
          </>
        )
      }
    >
      {readOnly ? (
        <>
          <p className="dialog__text">
            {agent.description} Built-in agents stay as they are; customize a copy to change one.
          </p>
          <pre className="agent-instructions">{agent.instructions}</pre>
        </>
      ) : (
        <>
          <p className="dialog__text">
            Instructions are added to the model request when you select this agent in Chat. Stored on this computer.
          </p>
          <div className="form__row">
            <label className="field field--grow">
              <span className="field__label">Name</span>
              <input
                className="input"
                maxLength={80}
                autoFocus
                placeholder="e.g. Cover Letter Writer"
                value={form.name}
                onChange={(e) => set({ name: e.target.value })}
              />
            </label>
          </div>
          <label className="field">
            <span className="field__label">Short description (optional)</span>
            <input
              className="input"
              maxLength={300}
              placeholder="What it helps with"
              value={form.description}
              onChange={(e) => set({ description: e.target.value })}
            />
          </label>
          <fieldset className="field">
            <legend className="field__label">Icon</legend>
            <div className="icon-picker" role="radiogroup" aria-label="Icon">
              {AGENT_ICONS.map(({ id, label, icon: Icon }) => (
                <button
                  key={id}
                  type="button"
                  role="radio"
                  aria-checked={form.icon === id}
                  aria-label={label}
                  title={label}
                  className={form.icon === id ? 'icon-picker__option icon-picker__option--selected' : 'icon-picker__option'}
                  onClick={() => set({ icon: id })}
                >
                  <Icon />
                </button>
              ))}
            </div>
          </fieldset>
          <FormField label="Instructions" hint={`${form.instructions.length.toLocaleString()} / 12,000 characters`}>
            {(control) => (
              <textarea
                {...control}
                className="input input--textarea agent-form__instructions"
                rows={10}
                maxLength={12000}
                placeholder={'How should the model behave? For example:\n- Write in a warm, concise tone\n- Always ask for the job posting first'}
                value={form.instructions}
                onChange={(e) => set({ instructions: e.target.value })}
              />
            )}
          </FormField>
        </>
      )}
    </Dialog>
  );
}
