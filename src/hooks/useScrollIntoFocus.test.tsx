// @vitest-environment jsdom
import '../test/dom';

import { render, waitFor } from '@testing-library/react';
import { useRef } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { useScrollIntoFocus } from './useScrollIntoFocus';

function Section({ focus }: { focus: boolean }) {
  const ref = useRef<HTMLElement>(null);
  useScrollIntoFocus(ref, focus);
  return <section ref={ref}>Connectors</section>;
}

describe('useScrollIntoFocus', () => {
  afterEach(() => vi.restoreAllMocks());

  it('scrolls to a section on a page that has already settled', async () => {
    const scroll = vi.spyOn(Element.prototype, 'scrollIntoView').mockImplementation(() => {});
    // Opened from a link while the page is already shown: nothing moves.
    const { rerender } = render(<Section focus={false} />);
    expect(scroll).not.toHaveBeenCalled();
    rerender(<Section focus />);
    await waitFor(() => expect(scroll).toHaveBeenCalledWith({ block: 'start' }));
  });
});
