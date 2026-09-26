import { BrandMark } from './BrandMark';

/** The app icon with the ReMa name (Settings → About). */
export function BrandLogo({ height = 32 }: { height?: number }) {
  return (
    <span className="brand-logo" role="img" aria-label="ReMa" style={{ fontSize: height * 0.56 }}>
      <BrandMark size={height} />
      <span className="brand-logo__name" aria-hidden="true">
        ReMa
      </span>
    </span>
  );
}
