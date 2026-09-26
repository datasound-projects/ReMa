import { useId } from 'react';

import mark from '../../assets/brand/mark.json';
import { useTheme } from '../../hooks/useTheme';

/** A lit palette from mark.json (`glow` only on dark surfaces). */
interface LitPalette {
  loop: string[];
  path: string[];
  node: string;
  gloss: number;
  glow?: string;
}

interface BrandMarkProps {
  size?: number;
  /**
   * `nav`: flat, two tones, for 16–32 px (title bar, rails). `full`: the lit
   * mark with its gradients (and, on dark surfaces, a soft glow) for larger
   * sizes such as the launch intro.
   */
  variant?: 'nav' | 'full';
}

/**
 * The ReMa mark: an abstract R of two forms (the loop and the path forward)
 * meeting at a node. Drawn from the master geometry in
 * `assets/brand/mark.json`, in the colors of the current theme.
 */
export function BrandMark({ size = 28, variant = 'nav' }: BrandMarkProps) {
  const dark = useTheme() === 'dark';
  const id = useId().replace(/[^a-zA-Z0-9_-]/g, '');
  const { palettes } = mark;

  let loop: string;
  let path: string;
  let node: string;
  let defs = null;
  let glossy = false;
  let glow = false;
  if (variant === 'nav') {
    const p = dark ? palettes.navDark : palettes.navLight;
    ({ loop, path, node } = p);
  } else {
    const p: LitPalette = dark ? palettes.dark : palettes.light;
    const glowColor = p.glow;
    loop = `url(#${id}-loop)`;
    path = `url(#${id}-path)`;
    node = p.node;
    glossy = true;
    glow = !!glowColor;
    defs = (
      <defs>
        <linearGradient id={`${id}-loop`} gradientUnits="userSpaceOnUse" x1="28" y1="20" x2="92" y2="100">
          <stop offset="0" stopColor={p.loop[0]} />
          <stop offset="1" stopColor={p.loop[1]} />
        </linearGradient>
        <linearGradient id={`${id}-path`} gradientUnits="userSpaceOnUse" x1="58" y1="68" x2="92" y2="100">
          <stop offset="0" stopColor={p.path[0]} />
          <stop offset="1" stopColor={p.path[1]} />
        </linearGradient>
        <linearGradient id={`${id}-gloss`} gradientUnits="userSpaceOnUse" x1="0" y1="18" x2="0" y2="62">
          <stop offset="0" stopColor="#fff" stopOpacity={p.gloss} />
          <stop offset="1" stopColor="#fff" stopOpacity="0" />
        </linearGradient>
        {glowColor && (
          <filter id={`${id}-glow`} x="-30%" y="-30%" width="160%" height="160%">
            <feGaussianBlur in="SourceAlpha" stdDeviation="3.2" result="blur" />
            <feFlood floodColor={glowColor} floodOpacity="0.45" />
            <feComposite in2="blur" operator="in" result="glow" />
            <feMerge>
              <feMergeNode in="glow" />
              <feMergeNode in="SourceGraphic" />
            </feMerge>
          </filter>
        )}
      </defs>
    );
  }

  return (
    <svg
      className="brand-mark"
      width={size}
      height={size}
      viewBox={`0 0 ${mark.size} ${mark.size}`}
      aria-hidden="true"
      focusable="false"
    >
      {defs}
      <mask id={`${id}-clear`} maskUnits="userSpaceOnUse" x="0" y="0" width={mark.size} height={mark.size}>
        <rect width={mark.size} height={mark.size} fill="#fff" />
        <circle cx={mark.node.cx} cy={mark.node.cy} r={mark.clearing} fill="#000" />
      </mask>
      <g filter={glow ? `url(#${id}-glow)` : undefined}>
        <g
          mask={`url(#${id}-clear)`}
          fill="none"
          strokeWidth={mark.stroke}
          strokeLinecap="round"
          strokeLinejoin="round"
        >
          <path d={mark.loop} stroke={loop} />
          <path d={mark.path} stroke={path} />
          {glossy && (
            <>
              <path d={mark.loop} stroke={`url(#${id}-gloss)`} />
              <path d={mark.path} stroke={`url(#${id}-gloss)`} />
            </>
          )}
        </g>
        <circle cx={mark.node.cx} cy={mark.node.cy} r={mark.node.r} fill={node} />
      </g>
    </svg>
  );
}
