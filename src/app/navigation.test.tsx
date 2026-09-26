// @vitest-environment jsdom
import '../test/dom';

import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import { NavigationContext } from './navigation';
import { FOOTER_NAV, MAIN_NAV } from './pages';
import { SettingsPage } from '../pages/SettingsPage';

describe('navigation', () => {
  it('places Agents directly below Chat', () => {
    expect(MAIN_NAV.map((i) => i.label)).toEqual(['Chat', 'Agents', 'Scheduled Tasks', 'Profile']);
    expect(FOOTER_NAV.map((i) => i.label)).toEqual(['Settings']);
  });

  it('manages MCP servers in Settings, not agents', () => {
    // Outside the desktop app the backend is unavailable: sections show
    // their error states, which is enough to see what Settings contains.
    render(
      <NavigationContext value={{ view: { page: 'settings' }, navigate: () => {} }}>
        <SettingsPage />
      </NavigationContext>,
    );
    const headings = screen.getAllByRole('heading', { level: 2 }).map((h) => h.textContent);
    expect(headings.some((h) => h?.startsWith('MCP'))).toBe(true);
    expect(headings.some((h) => /agent/i.test(h ?? ''))).toBe(false);
  });
});
