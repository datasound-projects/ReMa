import { useEffect, useRef, useState } from 'react';

import { toApiError } from '../../services/ipc';
import {
  saveMcpServer,
  testMcpServer,
  type McpAuth,
  type McpServer,
  type McpServerInput,
  type McpTestResult,
  type McpTransport,
} from '../../services/mcpService';
import { PlusIcon, TrashIcon } from '../icons';
import { Dialog } from '../ui/Dialog';
import { FormField } from '../ui/FormField';
import { IconButton } from '../ui/IconButton';

interface EnvRow {
  name: string;
  /** `null`: keep the stored value (existing variables). */
  value: string | null;
}

interface Form {
  name: string;
  transport: McpTransport;
  command: string;
  /** One argument per line (never split on spaces). */
  args: string;
  env: EnvRow[];
  cwd: string;
  url: string;
  auth: McpAuth;
  headerName: string;
  /** Empty with a stored secret: keep it. */
  secret: string;
}

const fromServer = (s: McpServer | null): Form =>
  s
    ? {
        name: s.name,
        transport: s.transport,
        command: s.command,
        args: s.args.join('\n'),
        env: s.envNames.map((name) => ({ name, value: null })),
        cwd: s.cwd,
        url: s.url,
        auth: s.auth,
        headerName: s.headerName,
        secret: '',
      }
    : {
        name: '',
        transport: 'stdio',
        command: '',
        args: '',
        env: [],
        cwd: '',
        url: '',
        auth: 'none',
        headerName: 'X-API-Key',
        secret: '',
      };

function toInput(form: Form): McpServerInput {
  return {
    name: form.name,
    transport: form.transport,
    command: form.command,
    args: form.args.split('\n').filter((a) => a.trim() !== ''),
    env: form.env.filter((e) => e.name.trim() !== ''),
    cwd: form.cwd,
    url: form.url,
    auth: form.auth,
    headerName: form.headerName,
    secret: form.secret === '' ? null : form.secret,
  };
}

const AUTHS: { id: McpAuth; label: string }[] = [
  { id: 'none', label: 'None' },
  { id: 'oauth', label: 'Sign in with OAuth' },
  { id: 'bearer', label: 'Bearer token' },
  { id: 'header', label: 'API key header' },
];

/**
 * Adds or edits an MCP server. Only the fields of the chosen connection
 * type are shown. Secrets go to the system keychain and are never shown
 * again; a new server starts turned off.
 */
export function McpServerDialog({ server, onClose }: { server: McpServer | null; onClose: () => void }) {
  const [form, setForm] = useState<Form>(() => fromServer(server));
  const [busy, setBusy] = useState<'save' | 'test' | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [test, setTest] = useState<McpTestResult | null>(null);
  const testRef = useRef<HTMLDivElement>(null);
  // The result can be below the fold of a long form.
  useEffect(() => {
    if (test) testRef.current?.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
  }, [test]);
  const set = (patch: Partial<Form>) => {
    setForm((f) => ({ ...f, ...patch }));
    setTest(null);
    setError(null);
  };
  // Renaming a stored variable needs its value again.
  const setEnv = (index: number, patch: Partial<EnvRow>) =>
    set({
      env: form.env.map((e, i) =>
        i === index ? { ...e, ...patch, value: patch.value ?? (patch.name !== undefined && e.value === null ? '' : e.value) } : e,
      ),
    });

  const storedSecret = server?.hasSecret === true && server.auth === form.auth;
  const needsSecret = form.transport === 'http' && (form.auth === 'bearer' || form.auth === 'header');

  const run = async (kind: 'save' | 'test') => {
    setBusy(kind);
    setError(null);
    try {
      if (kind === 'test') {
        setTest(await testMcpServer(server?.id ?? null, toInput(form)));
      } else {
        await saveMcpServer(server?.id ?? null, toInput(form));
        onClose();
        return;
      }
    } catch (err) {
      setError(toApiError(err).message);
    }
    setBusy(null);
  };

  return (
    <Dialog
      title={server ? `Edit ${server.name}` : 'Add MCP server'}
      size="wide"
      onClose={onClose}
      actions={
        <>
          {error && <span className="form-error">{error}</span>}
          <button type="button" className="button button--secondary" disabled={busy !== null} onClick={() => void run('test')}>
            {busy === 'test' ? 'Testing…' : 'Test connection'}
          </button>
          <button type="button" className="button button--ghost" onClick={onClose}>
            Cancel
          </button>
          <button type="button" className="button button--primary" disabled={busy !== null || !form.name.trim()} onClick={() => void run('save')}>
            {busy === 'save' ? 'Saving…' : server ? 'Save' : 'Add server'}
          </button>
        </>
      }
    >
      {!server && (
        <p className="dialog__text">
          New servers start turned off. Turn one on in Settings, then pick it with + in a chat to let the model use its
          tools.
        </p>
      )}
      <label className="field">
        <span className="field__label">Name</span>
        <input
          className="input"
          maxLength={60}
          autoFocus
          placeholder="e.g. Local Files"
          value={form.name}
          onChange={(e) => set({ name: e.target.value })}
        />
      </label>
      <fieldset className="field">
        <legend className="field__label">Connection</legend>
        <div className="segmented" role="radiogroup" aria-label="Connection type">
          {(
            [
              ['stdio', 'Local program'],
              ['http', 'Remote server (HTTP)'],
            ] as const
          ).map(([id, label]) => (
            <button
              key={id}
              type="button"
              role="radio"
              aria-checked={form.transport === id}
              className={form.transport === id ? 'segmented__option segmented__option--active' : 'segmented__option'}
              onClick={() => set({ transport: id })}
            >
              {label}
            </button>
          ))}
        </div>
      </fieldset>

      {form.transport === 'stdio' ? (
        <>
          <FormField label="Program" hint="A program name or an absolute path. Shells are not allowed.">
            {(control) => (
              <input
                {...control}
                className="input input--mono"
                spellCheck={false}
                placeholder="npx, uvx, node, or /absolute/path/to/server"
                value={form.command}
                onChange={(e) => set({ command: e.target.value })}
              />
            )}
          </FormField>
          <FormField label="Arguments" hint="One argument per line, passed exactly as written.">
            {(control) => (
              <textarea
                {...control}
                className="input input--textarea input--mono"
                rows={3}
                spellCheck={false}
                placeholder={'-y\n@modelcontextprotocol/server-filesystem\n/Users/me/Documents'}
                value={form.args}
                onChange={(e) => set({ args: e.target.value })}
              />
            )}
          </FormField>
          <div className="field">
            <span className="field__label">Environment variables</span>
            {form.env.map((row, index) => (
              <div key={index} className="form__row mcp-env">
                <input
                  className="input input--mono"
                  aria-label="Variable name"
                  placeholder="API_KEY"
                  spellCheck={false}
                  value={row.name}
                  onChange={(e) => setEnv(index, { name: e.target.value })}
                />
                <input
                  className="input input--mono"
                  type="password"
                  aria-label={`Value of ${row.name || 'variable'}`}
                  placeholder={row.value === null ? 'Stored in keychain (unchanged)' : 'Value'}
                  autoComplete="off"
                  value={row.value ?? ''}
                  onChange={(e) => setEnv(index, { value: e.target.value })}
                />
                <IconButton
                  label="Remove variable"
                  className="icon-button--small"
                  onClick={() => set({ env: form.env.filter((_, i) => i !== index) })}
                >
                  <TrashIcon />
                </IconButton>
              </div>
            ))}
            <div className="form__inline">
              <button
                type="button"
                className="button button--ghost entries__add"
                onClick={() => set({ env: [...form.env, { name: '', value: '' }] })}
              >
                <PlusIcon className="button__icon" />
                Add variable
              </button>
              <span className="form__hint">Values are kept in the system keychain. The server gets no other secrets.</span>
            </div>
          </div>
          <label className="field">
            <span className="field__label">Working folder (optional)</span>
            <input
              className="input input--mono"
              spellCheck={false}
              placeholder="Absolute path; your home folder if empty"
              value={form.cwd}
              onChange={(e) => set({ cwd: e.target.value })}
            />
          </label>
        </>
      ) : (
        <>
          <FormField label="Server URL" hint="HTTPS (plain HTTP only for this computer).">
            {(control) => (
              <input
                {...control}
                className="input input--mono"
                type="url"
                spellCheck={false}
                placeholder="https://example.com/mcp"
                value={form.url}
                onChange={(e) => set({ url: e.target.value })}
              />
            )}
          </FormField>
          <FormField
            label="Authentication"
            hint={form.auth === 'oauth' ? 'After saving, use Sign in: the server’s sign-in page opens in your browser.' : undefined}
          >
            {(control) => (
              <select
                {...control}
                className="input"
                value={form.auth}
                onChange={(e) => set({ auth: e.target.value as McpAuth, secret: '' })}
              >
                {AUTHS.map((a) => (
                  <option key={a.id} value={a.id}>
                    {a.label}
                  </option>
                ))}
              </select>
            )}
          </FormField>
          {form.auth === 'header' && (
            <label className="field">
              <span className="field__label">Header name</span>
              <input
                className="input input--mono"
                spellCheck={false}
                value={form.headerName}
                onChange={(e) => set({ headerName: e.target.value })}
              />
            </label>
          )}
          {needsSecret && (
            <label className="field">
              <span className="field__label">{form.auth === 'bearer' ? 'Token' : 'Header value'}</span>
              <input
                className="input input--mono"
                type="password"
                autoComplete="off"
                placeholder={storedSecret ? 'Stored in keychain (unchanged)' : ''}
                value={form.secret}
                onChange={(e) => set({ secret: e.target.value })}
              />
            </label>
          )}
        </>
      )}

      {test && (
        <div ref={testRef} className={test.ok ? 'notice mcp-test' : 'notice notice--danger mcp-test'} role="status">
          <p>{test.message}</p>
          {test.tools.length > 0 && (
            <p className="mcp-test__tools">{test.tools.map((t) => t.name).join(', ')}</p>
          )}
        </div>
      )}
    </Dialog>
  );
}
