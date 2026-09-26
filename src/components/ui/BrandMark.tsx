import icon from '../../assets/brand/rema-icon.png';

/**
 * The ReMa app icon (the glossy blue tile with the R), the same artwork as
 * the desktop icon. Decorative: the "ReMa" name is always next to it.
 */
export function BrandMark({ size = 28 }: { size?: number }) {
  return (
    <img className="brand-mark" src={icon} width={size} height={size} alt="" aria-hidden="true" draggable={false} />
  );
}
