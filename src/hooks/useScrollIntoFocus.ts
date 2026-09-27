import { useEffect, type RefObject } from 'react';

/**
 * Brings a section into view when `focus` is set (e.g. opened from a link),
 * and keeps it there while the sections above finish loading, until the
 * user scrolls, the layout settles or a few seconds pass.
 */
export function useScrollIntoFocus(ref: RefObject<HTMLElement | null>, focus: boolean) {
  useEffect(() => {
    const el = ref.current;
    if (!focus || !el) return;
    let stopped = false;
    let last = Number.NaN;
    let stableFrames = 0;
    const deadline = performance.now() + 4000;
    const stop = () => {
      stopped = true;
    };
    const events = ['wheel', 'touchstart', 'keydown', 'pointerdown'] as const;
    for (const name of events) window.addEventListener(name, stop, { passive: true, once: true });
    const tick = () => {
      if (stopped || performance.now() > deadline) return;
      const top = el.getBoundingClientRect().top;
      if (Math.abs(top - last) > 1) {
        el.scrollIntoView({ block: 'start' });
        stableFrames = 0;
      } else {
        stableFrames += 1;
      }
      last = el.getBoundingClientRect().top;
      if (stableFrames < 30) requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
    return () => {
      stopped = true;
      for (const name of events) window.removeEventListener(name, stop);
    };
  }, [ref, focus]);
}
