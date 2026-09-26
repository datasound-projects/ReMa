import type { CredentialKind, DocumentFormat, DocumentKind } from '../services/profileService';

export const FORMAT_LABELS: Record<DocumentFormat, string> = {
  pdf: 'PDF',
  docx: 'Word',
  text: 'Text',
  markdown: 'Markdown',
  png: 'PNG',
  jpeg: 'JPEG',
  webp: 'WebP',
};

export const KIND_LABELS: Record<DocumentKind, string> = {
  cv: 'CV',
  certificate: 'Credential',
  portfolio: 'Portfolio',
  other: 'Other',
};

export const CREDENTIAL_KINDS: { id: CredentialKind; label: string }[] = [
  { id: 'degree', label: 'Degree or diploma' },
  { id: 'professional_certificate', label: 'Professional certificate' },
  { id: 'course_certificate', label: 'Course certificate' },
  { id: 'training', label: 'Training' },
  { id: 'license', label: 'License' },
  { id: 'badge', label: 'Badge' },
  { id: 'other', label: 'Other evidence' },
];

export const credentialKindLabel = (kind: CredentialKind) =>
  CREDENTIAL_KINDS.find((k) => k.id === kind)?.label ?? 'Credential';

export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

/** `YYYY`, `YYYY-MM` or `YYYY-MM-DD` shown as e.g. "Jun 2024". */
export function formatPartialDate(value: string): string {
  const match = /^(\d{4})(?:-(\d{2}))?(?:-(\d{2}))?$/.exec(value.trim());
  if (!match) return value;
  const [, y, m, d] = match;
  if (!m) return y ?? value;
  const date = new Date(Number(y), Number(m) - 1, Number(d ?? 1));
  return date.toLocaleDateString(undefined, d ? { day: 'numeric', month: 'short', year: 'numeric' } : { month: 'short', year: 'numeric' });
}

/** Whether a `YYYY[-MM[-DD]]` date lies before today (end of its period). */
export function isPast(value: string, today = new Date()): boolean {
  const match = /^(\d{4})(?:-(\d{2}))?(?:-(\d{2}))?$/.exec(value.trim());
  if (!match) return false;
  const [, y, m, d] = match;
  const year = Number(y);
  const end = d
    ? new Date(year, Number(m) - 1, Number(d) + 1)
    : m
      ? new Date(year, Number(m), 1)
      : new Date(year + 1, 0, 1);
  return end.getTime() <= today.getTime();
}
