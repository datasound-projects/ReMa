import { useNavigation } from '../../app/navigation';
import {
  MAX_AGENTS,
  connectorOn,
  toggle,
  toggleConnector,
  type ChatSelection,
  type ConnectorOption,
} from '../../lib/chatSelection';
import type { Agent } from '../../services/agentService';
import type { ChatConnector } from '../../services/chatService';
import type { McpServer } from '../../services/mcpService';
import { AgentIcon } from '../agents/AgentIcon';
import { BriefcaseIcon, CalendarIcon, CheckIcon, MailIcon, PlugIcon, PlusIcon, SearchIcon } from '../icons';
import { Menu } from '../ui/Menu';

interface ChatToolsMenuProps {
  agents: Agent[];
  /** Servers turned on in Settings (the only ones that can be picked). */
  servers: McpServer[];
  selection: ChatSelection;
  onChange: (selection: ChatSelection) => void;
  /** ReMa MCP (built in) is on: it is in every chat, nothing to pick. */
  builtinOn?: boolean;
  /** Mail, calendars and applications this chat can turn on or off. */
  connectors?: ConnectorOption[];
}

const STATE_LABELS: Partial<Record<McpServer['status']['state'], string>> = {
  connected: 'Connected',
  connecting: 'Connecting…',
  needs_sign_in: 'Sign-in needed',
  error: 'Not working',
};

const CONNECTOR_ICONS: Record<ChatConnector, typeof MailIcon> = {
  gmail: MailIcon,
  outlook_mail: MailIcon,
  google_calendar: CalendarIcon,
  outlook_calendar: CalendarIcon,
  applications: BriefcaseIcon,
};

/** The composer's + button: agents, connectors and MCP servers for this chat. */
export function ChatToolsMenu({
  agents,
  servers,
  selection,
  onChange,
  builtinOn = false,
  connectors = [],
}: ChatToolsMenuProps) {
  const { navigate } = useNavigation();
  const builtins = agents.filter((a) => a.builtin);
  const mine = agents.filter((a) => !a.builtin);
  const count = selection.agentIds.length + selection.mcpServerIds.length;
  const full = selection.agentIds.length >= MAX_AGENTS;

  const agentItem = (agent: Agent) => {
    const checked = selection.agentIds.includes(agent.id);
    return (
      <button
        key={agent.id}
        type="button"
        role="menuitemcheckbox"
        aria-checked={checked}
        disabled={!checked && full}
        className="menu__item plus-menu__item"
        title={agent.description}
        onClick={() => onChange({ ...selection, agentIds: toggle(selection.agentIds, agent.id) })}
      >
        <AgentIcon icon={agent.icon} className="plus-menu__icon" />
        <span className="plus-menu__label">{agent.name}</span>
        {checked && <CheckIcon className="plus-menu__check" />}
      </button>
    );
  };

  return (
    <Menu
      align="start"
      placement="above"
      trigger={(props) => (
        <button
          type="button"
          className={count > 0 ? 'plus-button plus-button--active' : 'plus-button'}
          aria-label="Agents, connectors and MCP tools"
          title="Agents, connectors and MCP tools"
          {...props}
        >
          <PlusIcon />
        </button>
      )}
    >
      {(close) => (
        <div className="plus-menu">
          <div className="plus-menu__group" role="group" aria-label="Agents">
            <div className="plus-menu__heading">Agents</div>
            {builtins.map(agentItem)}
            {mine.length > 0 && <div className="plus-menu__subheading">My agents</div>}
            {mine.map(agentItem)}
            {full && <p className="plus-menu__hint">Up to {MAX_AGENTS} agents per chat.</p>}
            <button
              type="button"
              role="menuitem"
              className="menu__item plus-menu__link"
              onClick={() => {
                close();
                navigate({ page: 'agents' });
              }}
            >
              {mine.length > 0 ? 'Manage agents…' : 'My agents…'}
            </button>
          </div>
          <div className="plus-menu__group" role="group" aria-label="Connectors">
            <div className="plus-menu__heading">Connectors</div>
            {connectors.map((connector) => {
              const Icon = CONNECTOR_ICONS[connector.id];
              const checked = connector.ready && connectorOn(selection, connector.id);
              return (
                <button
                  key={connector.id}
                  type="button"
                  role="menuitemcheckbox"
                  aria-checked={checked}
                  disabled={!connector.ready}
                  className="menu__item plus-menu__item"
                  title={connector.ready ? undefined : 'Reconnect it in Settings → Connectors'}
                  onClick={() => onChange(toggleConnector(selection, connector.id, connectors))}
                >
                  <Icon className="plus-menu__icon" aria-hidden="true" />
                  <span className="plus-menu__label">{connector.name}</span>
                  {!connector.ready && <span className="plus-menu__state plus-menu__state--error">Needs attention</span>}
                  {checked && <CheckIcon className="plus-menu__check" />}
                </button>
              );
            })}
            {!connectors.some((c) => c.id !== 'applications') && (
              <p className="plus-menu__hint">Connect Gmail, Outlook or your calendar once in Settings.</p>
            )}
            <button
              type="button"
              role="menuitem"
              className="menu__item plus-menu__link"
              onClick={() => {
                close();
                navigate({ page: 'settings', focus: 'connectors' });
              }}
            >
              Connector settings…
            </button>
          </div>
          <div className="plus-menu__group" role="group" aria-label="Invoke MCP">
            <div className="plus-menu__heading">Invoke MCP</div>
            {builtinOn && (
              <div className="plus-menu__builtin" role="note" title="Turn it off in Settings → MCP">
                <SearchIcon className="plus-menu__icon" aria-hidden="true" />
                <span className="plus-menu__label">ReMa MCP</span>
                <span className="plus-menu__state">Built in · on in every chat</span>
              </div>
            )}
            {servers.length === 0 && (
              <p className="plus-menu__hint">
                {builtinOn
                  ? 'Add your own MCP servers in Settings.'
                  : 'No MCP servers are turned on. Add or turn one on in Settings.'}
              </p>
            )}
            {servers.map((server) => {
              const checked = selection.mcpServerIds.includes(server.id);
              const state = STATE_LABELS[server.status.state];
              return (
                <button
                  key={server.id}
                  type="button"
                  role="menuitemcheckbox"
                  aria-checked={checked}
                  className="menu__item plus-menu__item"
                  onClick={() => onChange({ ...selection, mcpServerIds: toggle(selection.mcpServerIds, server.id) })}
                >
                  <PlugIcon className="plus-menu__icon" />
                  <span className="plus-menu__label">{server.name}</span>
                  {state && <span className={`plus-menu__state plus-menu__state--${server.status.state}`}>{state}</span>}
                  {checked && <CheckIcon className="plus-menu__check" />}
                </button>
              );
            })}
            <button
              type="button"
              role="menuitem"
              className="menu__item plus-menu__link"
              onClick={() => {
                close();
                navigate({ page: 'settings', focus: 'mcp' });
              }}
            >
              MCP settings…
            </button>
          </div>
        </div>
      )}
    </Menu>
  );
}

/** Selected agents and servers as small removable chips. */
export function SelectionChips({
  agents,
  servers,
  selection,
  onChange,
}: {
  agents: Agent[];
  servers: McpServer[];
  selection: ChatSelection;
  onChange: (selection: ChatSelection) => void;
}) {
  const chosenAgents = selection.agentIds.flatMap((id) => agents.filter((a) => a.id === id));
  const chosenServers = selection.mcpServerIds.flatMap((id) => servers.filter((s) => s.id === id));
  if (chosenAgents.length === 0 && chosenServers.length === 0) return null;
  return (
    <div className="selection-chips" aria-label="Selected agents and MCP servers">
      {chosenAgents.map((agent) => (
        <span key={agent.id} className="selection-chip" title={agent.description}>
          <AgentIcon icon={agent.icon} className="selection-chip__icon" />
          {agent.name}
          <button
            type="button"
            className="selection-chip__remove"
            aria-label={`Remove ${agent.name}`}
            onClick={() => onChange({ ...selection, agentIds: selection.agentIds.filter((id) => id !== agent.id) })}
          >
            ×
          </button>
        </span>
      ))}
      {chosenServers.map((server) => (
        <span key={server.id} className="selection-chip selection-chip--mcp" title={`${server.name}: tools available to the model`}>
          <PlugIcon className="selection-chip__icon" />
          {server.name}
          <button
            type="button"
            className="selection-chip__remove"
            aria-label={`Remove ${server.name}`}
            onClick={() =>
              onChange({ ...selection, mcpServerIds: selection.mcpServerIds.filter((id) => id !== server.id) })
            }
          >
            ×
          </button>
        </span>
      ))}
    </div>
  );
}
