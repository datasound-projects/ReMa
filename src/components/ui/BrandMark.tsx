import markUrl from '../../assets/rema-mark.svg';

interface BrandMarkProps {
  size?: number;
}

/** The ReMa app mark. Same source as the desktop app icon. */
export function BrandMark({ size = 28 }: BrandMarkProps) {
  return <img className="brand-mark" src={markUrl} width={size} height={size} alt="" />;
}
