import { useState } from 'react';

import { toApiError } from '../../services/ipc';
import { respondToolApproval, type ApprovalDecision, type ToolActivity } from '../../services/chatService';
import { ChevronDownIcon, ChevronRightIcon, PlugIcon, ToolIcon } from '../icons';
import { isWebActivity } from '../../lib/toolActivity';
import { WebActivity } from './WebActivity';

const STATUS: Record<ToolActivity['status'], { label: string; tone: string }> = {
  awaiting_approval: { label: 'Needs your approval', tone: 'warning' },
  running: { label: 'Running…', tone: 'pending' },
  completed: { label: 'Done', tone: 'success' },
  failed: { label: 'Failed', tone: 'danger' },
  denied: { label: 'Declined', tone: 'muted' },
  unavailable: { label: 'Unavailable', tone: 'danger' },
};

function pretty(json: string): string {
  try {
    return JSON.stringify(JSON.parse(json), null, 2);
  } catch {
    return json;
  }
}

/**
 * What the model used while answering: its web searches, and MCP tools
 * (with approval for the ones that need it).
 */
export function ToolActivityList({ messageId, activity }: { messageId: number; activity: ToolActivity[] }) {
  if (activity.length === 0) return null;
  const web = activity.filter(isWebActivity);
  const tools = activity.filter((a) => !isWebActivity(a));
  return (
    <div className="tool-activity" aria-label="Tools used">
      {web.length > 0 && <WebActivity activity={web} />}
      {tools.map((a) =>
        a.status === 'unavailable' ? (
          <p key={a.id} className="tool-activity__notice" role="status">
            <PlugIcon aria-hidden="true" />
            <span>
              <strong>{a.server}</strong> could not be used{a.detail ? `: ${a.detail}` : '.'}
            </span>
          </p>
        ) : (
          <ToolCall key={a.id} messageId={messageId} activity={a} />
        ),
      )}
    </div>
  );
}

function ToolCall({ messageId, activity: a }: { messageId: number; activity: ToolActivity }) {
  const waiting = a.status === 'awaiting_approval';
  const [open, setOpen] = useState(false);
  const [sending, setSending] = useState<ApprovalDecision | null>(null);
  const [error, setError] = useState<string | null>(null);
  const status = STATUS[a.status];

  const answer = async (decision: ApprovalDecision) => {
    setSending(decision);
    setError(null);
    try {
      await respondToolApproval(messageId, a.id, decision);
    } catch (err) {
      setError(toApiError(err).message);
      setSending(null);
    }
  };

  return (
    <div className={waiting ? 'tool-call tool-call--waiting' : 'tool-call'}>
      <button
        type="button"
        className="tool-call__head"
        aria-expanded={open || waiting}
        onClick={() => setOpen(!open)}
        disabled={waiting}
      >
        {!waiting && (open ? <ChevronDownIcon className="tool-call__chevron" /> : <ChevronRightIcon className="tool-call__chevron" />)}
        <ToolIcon className="tool-call__icon" aria-hidden="true" />
        <span className="tool-call__name">
          <span className="tool-call__server">{a.server}</span>
          <code className="tool-call__tool">{a.tool}</code>
        </span>
        <span className={`tool-call__status tool-call__status--${status.tone}`}>{status.label}</span>
      </button>
      {(open || waiting) && (
        <div className="tool-call__body">
          {waiting && (
            <p className="tool-call__ask">
              The model wants to run <code>{a.tool}</code> on <strong>{a.server}</strong>. This server does not mark the
              tool as read-only, so it may change something.
            </p>
          )}
          {a.arguments && a.arguments !== '{}' && (
            <>
              <span className="tool-call__label">Arguments</span>
              <pre className="tool-call__pre">{pretty(a.arguments)}</pre>
            </>
          )}
          {a.detail && !waiting && (
            <>
              <span className="tool-call__label">{a.status === 'completed' ? 'Result' : 'Details'}</span>
              <pre className="tool-call__pre">{a.detail}</pre>
            </>
          )}
          {waiting && (
            <div className="tool-call__actions">
              <button
                type="button"
                className="button button--primary button--small"
                disabled={sending !== null}
                onClick={() => void answer('allow')}
              >
                {sending === 'allow' ? 'Allowing…' : 'Allow once'}
              </button>
              <button
                type="button"
                className="button button--secondary button--small"
                disabled={sending !== null}
                title="Allow this tool without asking again in this chat, until ReMa quits"
                onClick={() => void answer('allow_for_chat')}
              >
                Allow for this chat
              </button>
              <button
                type="button"
                className="button button--ghost button--small"
                disabled={sending !== null}
                onClick={() => void answer('deny')}
              >
                Deny
              </button>
            </div>
          )}
          {error && <p className="form-error">{error}</p>}
        </div>
      )}
    </div>
  );
}
