import { useEffect, useState, type CSSProperties } from 'react';

import { useCoverBrowser } from '../../app/browser';
import { BrandMark } from '../ui/BrandMark';

/** When the intro starts to fade out, and how long the fade takes (≈ 3 s in total). */
const HOLD_MS = 2350;
const FADE_MS = 600;
/** Skipping (a key or a click) fades out a little faster. */
const SKIP_FADE_MS = 350;

/**
 * Set on the first mount and never reset: the intro plays once per launch of
 * the app (this page load). Navigation, panels, minimizing or re-renders
 * never bring it back; only a fresh start of ReMa does.
 */
let playedThisLaunch = false;

type Phase = 'show' | 'leave' | 'done';

/**
 * The launch intro: "ReMa — Your Career Agent" over a plain canvas (warm
 * white, or warm charcoal in the dark theme), drawn above the already-mounted
 * app, then faded out and removed.
 */
export function LaunchIntro() {
  const [phase, setPhase] = useState<Phase>(() => (playedThisLaunch ? 'done' : 'show'));
  const [fadeMs, setFadeMs] = useState(FADE_MS);
  const [reduced] = useState(() => window.matchMedia('(prefers-reduced-motion: reduce)').matches);
  // A web page in the browser panel is drawn natively above ReMa; hide it meanwhile.
  useCoverBrowser(phase !== 'done');

  useEffect(() => {
    if (phase !== 'show') return;
    playedThisLaunch = true;
    const timer = window.setTimeout(() => setPhase('leave'), HOLD_MS);
    // Any key or click moves on at once (keys still reach the app underneath).
    const skip = () => {
      setFadeMs(SKIP_FADE_MS);
      setPhase('leave');
    };
    window.addEventListener('keydown', skip);
    window.addEventListener('pointerdown', skip);
    return () => {
      window.clearTimeout(timer);
      window.removeEventListener('keydown', skip);
      window.removeEventListener('pointerdown', skip);
    };
  }, [phase]);

  useEffect(() => {
    if (phase !== 'leave') return;
    const timer = window.setTimeout(() => setPhase('done'), fadeMs);
    return () => window.clearTimeout(timer);
  }, [phase, fadeMs]);

  if (phase === 'done') return null;

  const className = [
    'launch-intro',
    reduced && 'launch-intro--reduced',
    phase === 'leave' && 'launch-intro--leaving',
  ]
    .filter(Boolean)
    .join(' ');

  return (
    <div className={className} style={{ '--intro-fade': `${fadeMs}ms` } as CSSProperties} aria-hidden="true">
      <div className="launch-intro__glow" />
      <div className="launch-intro__content">
        <div className="launch-intro__brand">
          <BrandMark size={44} variant="full" />
          <span className="launch-intro__wordmark">ReMa</span>
        </div>
        <p className="launch-intro__headline">
          <span className="launch-intro__line">Your Career</span>
          <span className="launch-intro__line launch-intro__line--accent">Agent</span>
        </p>
      </div>
    </div>
  );
}
