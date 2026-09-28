/**
 * Header photos: a chosen image file becomes a small square JPEG data URL
 * (kept inside the document), and templates with round photos get a
 * circle-masked PNG made from it. Browser only (canvas).
 */

const SIDE = 480;
const MAX_BYTES = 400 * 1024;

function loadImage(src: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const image = new Image();
    image.onload = () => resolve(image);
    image.onerror = () => reject(new Error('The image could not be read.'));
    image.src = src;
  });
}

function readFile(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result));
    reader.onerror = () => reject(new Error('The file could not be read.'));
    reader.readAsDataURL(file);
  });
}

/** A square, center-cropped JPEG of the chosen image, small enough to store. */
export async function photoFromFile(file: File): Promise<string> {
  if (!/^image\/(png|jpeg|webp)$/.test(file.type)) throw new Error('Choose a PNG, JPEG or WebP image.');
  const image = await loadImage(await readFile(file));
  const side = Math.min(image.naturalWidth, image.naturalHeight);
  const sx = (image.naturalWidth - side) / 2;
  const sy = (image.naturalHeight - side) / 2;
  const canvas = document.createElement('canvas');
  canvas.width = SIDE;
  canvas.height = SIDE;
  const ctx = canvas.getContext('2d');
  if (!ctx) throw new Error('The image could not be processed.');
  ctx.fillStyle = '#ffffff';
  ctx.fillRect(0, 0, SIDE, SIDE);
  ctx.drawImage(image, sx, sy, side, side, 0, 0, SIDE, SIDE);
  for (const quality of [0.86, 0.72, 0.58, 0.45]) {
    const url = canvas.toDataURL('image/jpeg', quality);
    if (url.length <= MAX_BYTES * 1.37) return url;
  }
  throw new Error('The image is too large even after resizing.');
}

const circles = new Map<string, Promise<string>>();

/** The photo masked to a circle (PNG with transparency), cached per photo. */
export function circlePhoto(photo: string): Promise<string> {
  const cached = circles.get(photo);
  if (cached) return cached;
  const made = (async () => {
    const image = await loadImage(photo);
    const side = Math.min(image.naturalWidth, image.naturalHeight, SIDE);
    const canvas = document.createElement('canvas');
    canvas.width = side;
    canvas.height = side;
    const ctx = canvas.getContext('2d');
    if (!ctx) throw new Error('The image could not be processed.');
    ctx.beginPath();
    ctx.arc(side / 2, side / 2, side / 2, 0, Math.PI * 2);
    ctx.closePath();
    ctx.clip();
    ctx.drawImage(image, 0, 0, side, side);
    return canvas.toDataURL('image/png');
  })();
  circles.set(photo, made);
  made.catch(() => circles.delete(photo));
  return made;
}
