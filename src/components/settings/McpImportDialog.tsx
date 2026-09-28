import { useEffect, useState } from 'react';

import { toApiError } from '../../services/ipc';
import {
  importMcpServers,
  mcpImportSources,
  previewMcpImport,
  type McpImportCandidate,
  type McpImportResult,
  type McpImportSource,
} from '../../services/mcpService';
import { Dialog } from '../ui/Dialog';

/** Where the servers come from: an app's settings file, or pasted JSON. */
type Origin = { kind: 'app'; source: McpImportSource } | { kind: 'json' };

const PLACEHOLDER = `{
  "mcpServers": {
    "github": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-github"],
      "env": { "GITHUB_PERSONAL_ACCESS_TOKEN": "\${GITHUB_TOKEN}" }
    }
  }
}`;

const importable = (c: McpImportCandidate) => !c.problem && !c.alreadyAdded;

/**
 * Adds MCP servers from Claude Desktop, Claude Code, Cursor, VS Code or
 * Windsurf, or from pasted JSON (like `claude mcp add-json`). `${VAR}` is
 * filled in from the user's environment; values go to the keychain.
 */
export function McpImportDialog({ onClose }: { onClose: () => void }) {
  const [sources, setSources] = useState<McpImportSource[] | null>(null);
  const [origin, setOrigin] = useState<Origin>({ kind: 'json' });
  const [json, setJson] = useState('');
  const [pasted, setPasted] = useState<McpImportCandidate[] | null>(null);
  const [chosen, setChosen] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<McpImportResult | null>(null);

  useEffect(() => {
    let active = true;
    mcpImportSources()
      .then((found) => {
        if (!active) return;
        setSources(found);
        const first = found[0];
        if (first) {
          setOrigin({ kind: 'app', source: first });
          setChosen(new Set(first.servers.filter(importable).map((s) => s.name)));
        }
      })
      .catch(() => active && setSources([]));
    return () => {
      active = false;
    };
  }, []);

  const candidates = origin.kind === 'app' ? origin.source.servers : (pasted ?? []);

  const pick = (next: Origin) => {
    setOrigin(next);
    setResult(null);
    setError(null);
    const list = next.kind === 'app' ? next.source.servers : (pasted ?? []);
    setChosen(new Set(list.filter(importable).map((s) => s.name)));
  };

  const preview = async () => {
    setBusy(true);
    setError(null);
    try {
      const found = await previewMcpImport(json);
      setPasted(found);
      setChosen(new Set(found.filter(importable).map((s) => s.name)));
    } catch (err) {
      setPasted(null);
      setError(toApiError(err).message);
    } finally {
      setBusy(false);
    }
  };

  const add = async () => {
    setBusy(true);
    setError(null);
    try {
      setResult(
        await importMcpServers({
          app: origin.kind === 'app' ? origin.source.app : null,
          json: origin.kind === 'json' ? json : null,
          names: [...chosen],
        }),
      );
    } catch (err) {
      setError(toApiError(err).message);
    } finally {
      setBusy(false);
    }
  };

  const toggle = (name: string) =>
    setChosen((current) => {
      const next = new Set(current);
      if (next.has(name)) next.delete(name);
      else next.add(name);
      return next;
    });

  return (
    <Dialog
      title="Import MCP servers"
      size="wide"
      onClose={onClose}
      actions={
        <>
          {error && <span className="form-error">{error}</span>}
          <button type="button" className="button button--ghost" onClick={onClose}>
            {result ? 'Done' : 'Cancel'}
          </button>
          {!result && (
            <button
              type="button"
              className="button button--primary"
              disabled={busy || chosen.size === 0}
              onClick={() => void add()}
            >
              {busy ? 'Adding…' : chosen.size === 1 ? 'Add 1 server' : `Add ${chosen.size} servers`}
            </button>
          )}
        </>
      }
    >
      <p className="dialog__text">
        Add the servers you already use in other apps. Values such as tokens are filled in from your environment and
        stored in your system keychain.
      </p>
      <div className="segmented mcp-import__origins" role="radiogroup" aria-label="Import from">
        {(sources ?? []).map((source) => (
          <button
            key={source.app}
            type="button"
            role="radio"
            aria-checked={origin.kind === 'app' && origin.source.app === source.app}
            className={
              origin.kind === 'app' && origin.source.app === source.app
                ? 'segmented__option segmented__option--active'
                : 'segmented__option'
            }
            onClick={() => pick({ kind: 'app', source })}
          >
            {source.label}
          </button>
        ))}
        <button
          type="button"
          role="radio"
          aria-checked={origin.kind === 'json'}
          className={origin.kind === 'json' ? 'segmented__option segmented__option--active' : 'segmented__option'}
          onClick={() => pick({ kind: 'json' })}
        >
          Paste JSON
        </button>
      </div>
      {sources?.length === 0 && (
        <p className="field__hint">No MCP servers were found in Claude Desktop, Claude Code, Cursor, VS Code or Windsurf.</p>
      )}
      {origin.kind === 'app' && <p className="field__hint mcp-import__path">{origin.source.path}</p>}
      {origin.kind === 'json' && (
        <label className="field">
          <span className="field__label">MCP configuration</span>
          <textarea
            className="input input--mono mcp-import__json"
            rows={8}
            spellCheck={false}
            placeholder={PLACEHOLDER}
            value={json}
            onChange={(e) => {
              setJson(e.target.value);
              setPasted(null);
            }}
          />
          <button
            type="button"
            className="button button--secondary button--small mcp-import__preview"
            disabled={busy || !json.trim()}
            onClick={() => void preview()}
          >
            Show servers
          </button>
        </label>
      )}

      {result ? (
        <div className="notice mcp-import__result" role="status">
          <p>
            {result.added.length === 0
              ? 'No servers were added.'
              : `Added ${result.added.map((s) => s.name).join(', ')}.`}
          </p>
          {result.skipped.map((skip) => (
            <p key={skip.name} className="mcp-import__skip">
              {skip.name}: {skip.reason}
            </p>
          ))}
        </div>
      ) : (
        candidates.length > 0 && (
          <ul className="mcp-import__list" aria-label="Servers">
            {candidates.map((c) => {
              const note = c.problem ?? (c.alreadyAdded ? 'Already in ReMa.' : c.disabled ? 'Off in that app; added turned off.' : null);
              return (
                <li key={c.name} className="mcp-import__item">
                  <label className="checkbox">
                    <input
                      type="checkbox"
                      disabled={!importable(c)}
                      checked={chosen.has(c.name)}
                      onChange={() => toggle(c.name)}
                    />
                    <span className="mcp-import__name">{c.name}</span>
                  </label>
                  <span className="mcp-import__summary">{c.summary}</span>
                  {c.envNames.length > 0 && (
                    <span className="mcp-import__env">Variables: {c.envNames.join(', ')}</span>
                  )}
                  {note && <span className={c.problem ? 'mcp-import__problem' : 'mcp-import__note'}>{note}</span>}
                </li>
              );
            })}
          </ul>
        )
      )}
    </Dialog>
  );
}
