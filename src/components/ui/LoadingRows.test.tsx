// @vitest-environment jsdom
import '../../test/dom';

import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import { LoadFailed, LoadingRows } from './LoadingRows';

describe('Settings loading placeholders', () => {
  it('shows placeholder rows with a label for assistive technology', () => {
    render(<LoadingRows count={3} label="Loading providers…" />);
    const status = screen.getByRole('status', { name: 'Loading providers…' });
    expect(status.querySelectorAll('.loading-rows__row').length).toBe(3);
  });

  it('offers to try again after a failed load', () => {
    const retry = vi.fn();
    render(<LoadFailed message="The keychain did not answer." onRetry={retry} />);
    expect(screen.getByRole('alert').textContent).toContain('The keychain did not answer.');
    fireEvent.click(screen.getByRole('button', { name: 'Try again' }));
    expect(retry).toHaveBeenCalledTimes(1);
  });
});
