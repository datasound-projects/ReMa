import { useEffect, useRef, useState } from 'react';

import { useAction } from '../../hooks/useAction';
import { useMcpServers } from '../../hooks/useMcpServers';
import { toApiError } from '../../services/ipc';
import {
  cancelMcpSignIn,
  connectMcpServer,
  deleteMcpServer,
  disconnectMcpServer,
  setMcpServerEnabled,
  signInMcpServer,
  signOutMcpServer,
  testMcpServer,
  type McpServer,
  type McpTestResult,
} from '../../services/mcpService';
import { ChevronDownIcon, ChevronRightIcon, MoreIcon, PlugIcon, PlusIcon } from '../icons';
import { HelpTip } from '../ui/HelpTip';
import { Menu, type MenuItem } from '../ui/Menu';
import { StatusIndicator, type StatusTone } from '../ui/StatusIndicator';
import { Switch } from '../ui/Switch';
import { McpServerDialog } from './McpServerDialog';

export const MCP_HELP = 'MCP lets ReMa connect to external tools and data sources that AI models can use when you allow them.';

const STATUS: Record<McpServer['status']['state'], { tone: StatusTone; label: string }> = {
  disabled: { tone: 'idle', label: 'Off' },
  disconnected: { tone: 'idle', label: 'Ready' },
  connecting: { tone: 'pending', label: 'Connecting…' },
  connected: { tone: 'ready', label: 'Connected' },
  needs_sign_in: { tone: 'pending', label: 'Sign-in needed' },
  error: { tone: 'error', label: 'Connection failed' },
};

/** Settings → MCP: configure servers and turn them on or off. */
export function McpSection({ focus = false }: { focus?: boolean }) {
  const servers = useMcpServers();
  const [editing, setEditing] = useState<McpServer | 'new' | null>(null);
  const ref = useRef<HTMLElement>(null);

  useEffect(() => {
    if (focus) ref.current?.scrollIntoView({ behavior: 'smooth', block: 'start' });
  }, [focus]);

  const list = servers.state.status === 'success' ? servers.state.data : [];

  return (
    <section className="section mcp" aria-labelledby="mcp-heading" ref={ref} id="settings-mcp">
      <div className="section__head">
        <div className="section__heading">
          <h2 id="mcp-heading" className="section__title mcp__title">
            MCP
            <HelpTip text={MCP_HELP} label="What is MCP?" />
          </h2>
          <p className="section__description">Model Context Protocol</p>
        </div>
      </div>

      {servers.state.status === 'error' && (
        <p className="form-error" role="alert">
          {servers.state.error.message}
        </p>
      )}
      <div className="panel panel--list">
        {servers.state.status === 'success' && list.length === 0 && (
          <div className="mcp-empty">
            <PlugIcon aria-hidden="true" />
            <span>No MCP servers yet. Add a local program or a remote server to give models extra tools.</span>
          </div>
        )}
        {list.map((server) => (
          <McpServerRow key={server.id} server={server} onEdit={() => setEditing(server)} />
        ))}
        <div className="provider provider--add">
          <button type="button" className="button button--ghost entries__add" onClick={() => setEditing('new')}>
            <PlusIcon className="button__icon" />
            Add MCP server
          </button>
        </div>
      </div>

      {editing !== null && (
        <McpServerDialog
          key={editing === 'new' ? 'new' : editing.id}
          server={editing === 'new' ? null : editing}
          onClose={() => setEditing(null)}
        />
      )}
    </section>
  );
}

function McpServerRow({ server, onEdit }: { server: McpServer; onEdit: () => void }) {
  const action = useAction();
  const [test, setTest] = useState<McpTestResult | null>(null);
  const [testing, setTesting] = useState(false);
  const [signingIn, setSigningIn] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const [showTools, setShowTools] = useState(false);
  const status = STATUS[server.status.state];
  const tools = server.status.tools;
  const oauth = server.transport === 'http' && server.auth === 'oauth';

  const runTest = async () => {
    setTesting(true);
    setTest(null);
    try {
      setTest(
        await testMcpServer(server.id, {
          name: server.name,
          transport: server.transport,
          command: server.command,
          args: server.args,
          env: server.envNames.map((name) => ({ name, value: null })),
          cwd: server.cwd,
          url: server.url,
          auth: server.auth,
          headerName: server.headerName,
          secret: null,
        }),
      );
    } catch (err) {
      setTest({ ok: false, message: toApiError(err).message, tools: [] });
    } finally {
      setTesting(false);
    }
  };

  const signIn = async () => {
    setSigningIn(true);
    const ok = await action.run(() => signInMcpServer(server.id));
    setSigningIn(false);
    if (!ok) setTest(null);
  };

  const items: MenuItem[] = [
    ...(server.enabled
      ? server.status.state === 'connected'
        ? [
            { label: 'Reconnect', onSelect: () => void action.run(() => connectMcpServer(server.id)) },
            { label: 'Disconnect', onSelect: () => void action.run(() => disconnectMcpServer(server.id)) },
          ]
        : [{ label: 'Connect', onSelect: () => void action.run(() => connectMcpServer(server.id)) }]
      : []),
    ...(oauth
      ? [
          { label: 'Sign in…', onSelect: () => void signIn() },
          { label: 'Sign out', onSelect: () => void action.run(() => signOutMcpServer(server.id)) },
        ]
      : []),
    { label: 'Remove', danger: true, onSelect: () => setConfirming(true) },
  ];

  const type = server.transport === 'stdio' ? 'Local program' : 'Remote (HTTP)';
  const target = server.transport === 'stdio' ? [server.command, ...server.args].join(' ') : server.url;

  return (
    <div className="provider mcp-server">
      <div className="provider__row">
        <PlugIcon className="mcp-server__icon" aria-hidden="true" />
        <div className="mcp-server__main">
          <span className="provider__name">{server.name}</span>
          <span className="mcp-server__type" title={target}>
            {type} · <span className="mcp-server__target">{target}</span>
          </span>
        </div>
        <StatusIndicator tone={status.tone} label={signingIn ? 'Signing in…' : status.label} />
        <Switch
          checked={server.enabled}
          aria-label={server.enabled ? `Turn off ${server.name}` : `Turn on ${server.name}`}
          disabled={action.busy}
          onChange={(on) => void action.run(() => setMcpServerEnabled(server.id, on))}
        />
      </div>
      <div className="mcp-server__actions">
        {oauth && (signingIn || server.status.state === 'needs_sign_in') ? (
          signingIn ? (
            <button type="button" className="button button--ghost button--small" onClick={() => void cancelMcpSignIn(server.id)}>
              Cancel sign-in
            </button>
          ) : (
            <button type="button" className="button button--primary button--small" onClick={() => void signIn()}>
              Sign in
            </button>
          )
        ) : null}
        {server.enabled && server.status.state === 'error' && (
          <button
            type="button"
            className="button button--secondary button--small"
            disabled={action.busy}
            onClick={() => void action.run(() => connectMcpServer(server.id))}
          >
            Reconnect
          </button>
        )}
        <button type="button" className="button button--secondary button--small" disabled={testing} onClick={() => void runTest()}>
          {testing ? 'Testing…' : 'Test'}
        </button>
        <button type="button" className="button button--ghost button--small" onClick={onEdit}>
          Edit
        </button>
        <button type="button" className="button button--ghost button--small" onClick={() => setConfirming(true)}>
          Remove
        </button>
        <Menu
          items={items}
          trigger={(props) => (
            <button type="button" className="icon-button icon-button--small" aria-label="More actions" title="More actions" {...props}>
              <MoreIcon />
            </button>
          )}
        />
        {tools.length > 0 && (
          <button type="button" className="link-button mcp-server__tools-toggle" aria-expanded={showTools} onClick={() => setShowTools(!showTools)}>
            {showTools ? <ChevronDownIcon className="button__icon" /> : <ChevronRightIcon className="button__icon" />}
            {tools.length} tool{tools.length === 1 ? '' : 's'}
            {server.status.protocolVersion && <span className="mcp-server__version"> · MCP {server.status.protocolVersion}</span>}
          </button>
        )}
      </div>
      {server.status.message && server.status.state !== 'connected' && (
        <p className={server.status.state === 'error' ? 'form-error mcp-server__message' : 'form__hint mcp-server__message'}>
          {server.status.message}
        </p>
      )}
      {!server.enabled && <p className="form__hint mcp-server__message">Off: not offered in Chat and nothing runs.</p>}
      {showTools && (
        <ul className="mcp-tools">
          {tools.map((tool) => (
            <li key={tool.name} className="mcp-tools__item">
              <code>{tool.name}</code>
              {tool.readOnly && <span className="badge">Read-only</span>}
              <span className="mcp-tools__description">{tool.title ?? tool.description}</span>
            </li>
          ))}
        </ul>
      )}
      {test && (
        <p className={test.ok ? 'notice mcp-server__test' : 'notice notice--danger mcp-server__test'} role="status">
          {test.message}
        </p>
      )}
      {confirming && (
        <div className="doc-card__confirm" role="group" aria-label="Confirm removal">
          <span>Remove {server.name}? Its stored secrets are deleted and chats stop using it.</span>
          <button
            type="button"
            className="button button--danger button--small"
            disabled={action.busy}
            onClick={() => void action.run(() => deleteMcpServer(server.id))}
          >
            Remove
          </button>
          <button type="button" className="button button--ghost button--small" onClick={() => setConfirming(false)}>
            Cancel
          </button>
        </div>
      )}
      {action.error && <p className="form-error mcp-server__message">{action.error}</p>}
    </div>
  );
}
