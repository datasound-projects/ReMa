#!/usr/bin/env node
// A minimal MCP server for ReMa's tests (stdio, newline-delimited JSON-RPC).
//
// FAKE_MCP_ERA=modern  answers `server/discover` (protocol 2026-07-28);
//                      otherwise it is a legacy server that needs `initialize`.
// FAKE_MCP_CRASH=1     prints an error and exits at once.
//
// Tools: `echo` (read-only), `add_note` (changes something) and `env`
// (reports what it can see of its environment).

import { createInterface } from 'node:readline';

if (process.env.FAKE_MCP_CRASH === '1') {
  process.stderr.write('Error: missing configuration FAKE_MCP_TOKEN\n');
  process.exit(3);
}

const modern = process.env.FAKE_MCP_ERA === 'modern';
const serverInfo = { name: 'rema-test-server', version: '1.2.3' };
let initialized = false;

const tools = [
  {
    name: 'echo',
    title: 'Echo',
    description: 'Repeats the given text.',
    inputSchema: { type: 'object', properties: { text: { type: 'string' } }, required: ['text'] },
    annotations: { readOnlyHint: true },
  },
  {
    name: 'add_note',
    description: 'Saves a note.',
    inputSchema: { type: 'object', properties: { note: { type: 'string' } }, required: ['note'] },
    annotations: { readOnlyHint: false, destructiveHint: false },
  },
  {
    name: 'env',
    description: 'Reports environment variables the server can see.',
    inputSchema: { type: 'object', properties: {} },
    annotations: { readOnlyHint: true },
  },
];

function send(message) {
  process.stdout.write(`${JSON.stringify(message)}\n`);
}

function result(id, value) {
  send({ jsonrpc: '2.0', id, result: modern ? { resultType: 'complete', ...value } : value });
}

function error(id, code, message) {
  send({ jsonrpc: '2.0', id, error: { code, message } });
}

function text(id, value, isError = false) {
  result(id, { content: [{ type: 'text', text: value }], isError });
}

const lines = createInterface({ input: process.stdin });
lines.on('line', (line) => {
  let message;
  try {
    message = JSON.parse(line);
  } catch {
    return;
  }
  const { id, method, params = {} } = message;
  if (id === undefined || method === undefined) return; // notifications, responses
  switch (method) {
    case 'server/discover':
      if (!modern) return error(id, -32601, 'Method not found');
      return send({
        jsonrpc: '2.0',
        id,
        result: {
          resultType: 'complete',
          supportedVersions: ['2026-07-28'],
          capabilities: { tools: {} },
          ttlMs: 0,
          cacheScope: 'private',
          _meta: { 'io.modelcontextprotocol/serverInfo': serverInfo },
        },
      });
    case 'initialize':
      if (modern) return error(id, -32601, 'Unsupported: use server/discover (2026-07-28)');
      initialized = true;
      return result(id, {
        protocolVersion: params.protocolVersion ?? '2025-11-25',
        capabilities: { tools: {} },
        serverInfo,
      });
    case 'tools/list':
      if (!modern && !initialized) return error(id, -32002, 'Not initialized');
      return result(id, modern ? { tools, ttlMs: 0, cacheScope: 'private' } : { tools });
    case 'tools/call': {
      const args = params.arguments ?? {};
      if (params.name === 'echo') return text(id, `echo: ${args.text ?? ''}`);
      if (params.name === 'add_note') return text(id, `saved: ${args.note ?? ''}`);
      if (params.name === 'env') {
        return text(
          id,
          JSON.stringify({
            token: process.env.FAKE_MCP_TOKEN ?? null,
            parentSecret: process.env.REMA_PARENT_SECRET ?? null,
            hasPath: Boolean(process.env.PATH),
          }),
        );
      }
      return text(id, `unknown tool ${params.name}`, true);
    }
    default:
      return error(id, -32601, 'Method not found');
  }
});
