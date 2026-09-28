import { useEffect, useState } from 'react';

import { toApiError } from '../../services/ipc';
import type { McpImportApp } from '../../services/mcpService';
import { addRemaMcpTo, remaMcpLaunch } from '../../services/remaMcpService';
import { claudeCodeCommand, codexToml, mcpJson, type Launch } from '../../lib/remaMcpSnippets';
import { Dialog } from '../ui/Dialog';

const FILE_APPS: { app: McpImportApp; label: string }[] = [
  { app: 'claude_desktop', label: 'Claude Desktop' },
  { app: 'cursor', label: 'Cursor' },
  { app: 'vs_code', label: 'VS Code' },
];

function Snippet({ label, text }: { label: string; text: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <div className="rema-elsewhere__snippet">
      <div className="rema-elsewhere__snippet-head">
        <span className="field__label">{label}</span>
        <button
          type="button"
          className="button button--ghost button--small"
          onClick={() =>
            void navigator.clipboard
              ?.writeText(text)
              .then(() => setCopied(true))
              .catch(() => setCopied(false))
          }
        >
          {copied ? 'Copied' : 'Copy'}
        </button>
      </div>
      <pre className="rema-elsewhere__code">{text}</pre>
    </div>
  );
}

/**
 * ReMa MCP in Claude Desktop, Claude Code, Codex, Cursor or VS Code: they
 * start this ReMa's program with `mcp` and talk MCP over stdio.
 */
export function RemaMcpElsewhere({ onClose }: { onClose: () => void }) {
  const [launch, setLaunch] = useState<Launch | null | undefined>(undefined);
  const [added, setAdded] = useState<Partial<Record<McpImportApp, string>>>({});
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    remaMcpLaunch()
      .then((l) => active && setLaunch(l))
      .catch((err: unknown) => active && setError(toApiError(err).message));
    return () => {
      active = false;
    };
  }, []);

  const add = async (app: McpImportApp) => {
    setError(null);
    try {
      const path = await addRemaMcpTo(app);
      setAdded((a) => ({ ...a, [app]: path }));
    } catch (err) {
      setError(toApiError(err).message);
    }
  };

  return (
    <Dialog
      title="Use ReMa MCP in other apps"
      size="wide"
      onClose={onClose}
      actions={
        <button type="button" className="button button--primary" onClick={onClose}>
          Done
        </button>
      }
    >
      <p className="dialog__text">
        Other AI apps can search jobs with ReMa MCP too. They start ReMa in the background (no window opens) and use
        ReMa’s job sources and cache. Turning ReMa MCP off here turns it off there too.
      </p>
      {error && (
        <p className="form-error" role="alert">
          {error}
        </p>
      )}
      {launch === null && <p className="form-error">ReMa’s program could not be located on this computer.</p>}
      {launch && (
        <>
          <div className="rema-elsewhere__apps">
            {FILE_APPS.map(({ app, label }) => (
              <div key={app} className="rema-elsewhere__app">
                <button type="button" className="button button--secondary button--small" onClick={() => void add(app)}>
                  Add to {label}
                </button>
                {added[app] && (
                  <span className="rema-elsewhere__done" role="status">
                    Added to {added[app]}. Restart {label} to use it.
                  </span>
                )}
              </div>
            ))}
          </div>
          <Snippet label="Claude Code (run in a terminal)" text={claudeCodeCommand(launch)} />
          <Snippet label="Codex (~/.codex/config.toml)" text={codexToml(launch)} />
          <Snippet label="Any other MCP app (mcpServers JSON)" text={mcpJson(launch)} />
        </>
      )}
    </Dialog>
  );
}
