// @vitest-environment jsdom
import '../../test/dom';

import { render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import type { ToolActivity } from '../../services/chatService';
import { ToolActivityList } from './ToolActivityList';

vi.mock('../../services/chatService', () => ({ respondToolApproval: vi.fn(() => Promise.resolve(null)) }));

const waiting = (patch: Partial<ToolActivity>): ToolActivity => ({
  id: 'c1',
  serverId: null,
  server: 'Applications',
  tool: 'applications_update_status',
  status: 'awaiting_approval',
  arguments: '{"application_id":7,"status":"offer"}',
  detail: 'Change Globex — Data Engineer from In process to Offer',
  readOnly: false,
  kind: 'connector',
  sources: [],
  ...patch,
});

describe('tool approvals', () => {
  it('connector changes are approved one call at a time', () => {
    render(<ToolActivityList messageId={1} activity={[waiting({})]} />);
    expect(screen.getByText('Change Globex — Data Engineer from In process to Offer')).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Allow' })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Deny' })).toBeTruthy();
    expect(screen.queryByRole('button', { name: /Allow for this chat/ })).toBeNull();
  });

  it('after private data, an MCP call explains why and is approved once', () => {
    render(
      <ToolActivityList
        messageId={1}
        activity={[
          waiting({
            server: 'Notes',
            serverId: 4,
            tool: 'save_note',
            kind: 'mcp',
            detail: 'This answer has read your mail, calendar or applications. Allow only if you want Notes to receive what the model sends it.',
          }),
        ]}
      />,
    );
    expect(screen.getByText(/has read your mail, calendar or applications/)).toBeTruthy();
    expect(screen.queryByRole('button', { name: /Allow for this chat/ })).toBeNull();
  });

  it('other MCP calls can still be allowed for the chat', () => {
    render(
      <ToolActivityList
        messageId={1}
        activity={[waiting({ server: 'Notes', serverId: 4, tool: 'save_note', kind: 'mcp', detail: null })]}
      />,
    );
    expect(screen.getByRole('button', { name: /Allow for this chat/ })).toBeTruthy();
  });
});
