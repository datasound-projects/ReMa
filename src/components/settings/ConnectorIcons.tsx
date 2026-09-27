import type { ConnectorId } from '../../services/connectorService';

interface LogoProps {
  size?: number;
}

/** The Gmail product icon (Google brand colors). */
function GmailLogo({ size = 32 }: LogoProps) {
  return (
    <svg width={size} height={size} viewBox="52 42 88 66" aria-hidden="true">
      <path fill="#4285f4" d="M58 108h14V74L52 59v43c0 3.32 2.69 6 6 6" />
      <path fill="#34a853" d="M120 108h14c3.32 0 6-2.69 6-6V59l-20 15" />
      <path fill="#fbbc04" d="M120 48v26l20-15v-8c0-7.42-8.47-11.65-14.4-7.2" />
      <path fill="#ea4335" d="M72 74V48l24 18 24-18v26L96 92" />
      <path fill="#c5221f" d="M52 51v8l20 15V48l-5.6-4.2c-5.94-4.45-14.4-.22-14.4 7.2" />
    </svg>
  );
}

/** The Google Calendar product icon. */
function GoogleCalendarLogo({ size = 32 }: LogoProps) {
  return (
    <svg width={size} height={size} viewBox="0 0 200 200" aria-hidden="true">
      <path fill="#fff" d="M152 48l-48-5-59 5-5 54 5 53 54 7 53-7 5-55z" />
      <path
        fill="#1a73e8"
        d="M69 129c-4-3-7-7-8-12l9-4c1 3 2 6 4 8 2 1 5 2 8 2s6-1 8-3 3-4 3-7-1-5-3-7-5-3-9-3h-5v-9h5c3 0 5-1 7-2s3-4 3-7c0-2-1-4-3-6-2-1-4-2-7-2s-5 1-6 2-3 3-4 5l-9-4c1-3 3-6 7-9 3-3 7-4 12-4 4 0 7 1 10 2 3 1 5 3 7 6 2 3 3 5 3 9 0 3-1 6-2 8-2 2-4 4-6 5v1c3 1 6 3 7 6 2 3 3 6 3 9s-1 7-3 10-4 5-8 7-7 2-11 2c-5 0-9-1-13-4zM125 84l-10 7-5-8 18-13h7v62h-10z"
      />
      <path fill="#ea4335" d="M152 200l48-48-24-11-24 11-11 24z" />
      <path fill="#34a853" d="M37 176l11 24h104v-48H48z" />
      <path fill="#4285f4" d="M16 0C7 0 0 7 0 16v136l24 11 24-11V48h104l11-24L152 0z" />
      <path fill="#188038" d="M0 152v32c0 9 7 16 16 16h32v-48z" />
      <path fill="#fbbc04" d="M152 48v104h48V48l-24-11z" />
      <path fill="#1967d2" d="M200 48V16c0-9-7-16-16-16h-32v48z" />
    </svg>
  );
}

/** The Outlook "O" tile shared by both Microsoft connectors. */
function OutlookTile() {
  return (
    <>
      <rect x="2" y="9" width="16" height="15" rx="2" fill="#0364b8" />
      <ellipse cx="10" cy="16.5" rx="3.4" ry="4.2" fill="none" stroke="#fff" strokeWidth="2" />
    </>
  );
}

/** Microsoft Outlook (mail). */
function OutlookLogo({ size = 32 }: LogoProps) {
  return (
    <svg width={size} height={size} viewBox="0 0 32 32" aria-hidden="true">
      <path fill="#0078d4" d="M12 7h16.5A1.5 1.5 0 0 1 30 8.5v15a1.5 1.5 0 0 1-1.5 1.5H12z" />
      <path fill="#28a8ea" d="M12 7h16.5A1.5 1.5 0 0 1 30 8.5v.9l-9 6.1-9-6.1z" />
      <path fill="#50d9ff" d="M21 15.5l9-6.1V11l-9 6.2-9-6.2V9.4z" opacity="0.7" />
      <OutlookTile />
    </svg>
  );
}

/** Microsoft Outlook (calendar). */
function OutlookCalendarLogo({ size = 32 }: LogoProps) {
  const cells = [0, 1, 2].flatMap((row) => [0, 1, 2].map((col) => ({ row, col })));
  return (
    <svg width={size} height={size} viewBox="0 0 32 32" aria-hidden="true">
      <path fill="#0078d4" d="M12 7h16.5A1.5 1.5 0 0 1 30 8.5v15a1.5 1.5 0 0 1-1.5 1.5H12z" />
      <path fill="#28a8ea" d="M12 7h16.5A1.5 1.5 0 0 1 30 8.5V11.5H12z" />
      {cells.map(({ row, col }) => (
        <rect
          key={`${row}-${col}`}
          x={19 + col * 3.5}
          y={13.5 + row * 3.5}
          width="2.5"
          height="2.5"
          rx="0.4"
          fill="#fff"
          opacity={row === 1 && col === 1 ? 1 : 0.55}
        />
      ))}
      <OutlookTile />
    </svg>
  );
}

/** The official product icon of a connector. */
export function ConnectorLogo({ id, size }: { id: ConnectorId; size?: number }) {
  switch (id) {
    case 'gmail':
      return <GmailLogo size={size} />;
    case 'google_calendar':
      return <GoogleCalendarLogo size={size} />;
    case 'outlook_mail':
      return <OutlookLogo size={size} />;
    case 'outlook_calendar':
      return <OutlookCalendarLogo size={size} />;
  }
}
