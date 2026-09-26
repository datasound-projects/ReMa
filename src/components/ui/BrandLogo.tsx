import logoDark from '../../assets/brand/rema-logo-dark.svg';
import logoLight from '../../assets/brand/rema-logo-light.svg';
import { useTheme } from '../../hooks/useTheme';

/** The mark with the ReMa wordmark (About, onboarding). */
export function BrandLogo({ height = 32 }: { height?: number }) {
  const dark = useTheme() === 'dark';
  return <img className="brand-logo" src={dark ? logoDark : logoLight} height={height} alt="ReMa" />;
}
