import type { SVGProps } from 'react';

export type IconProps = SVGProps<SVGSVGElement>;

/** Base for 20×20 stroke icons; inherits color from `currentColor`. */
function Icon({ children, ...props }: IconProps) {
  return (
    <svg
      width={20}
      height={20}
      viewBox="0 0 20 20"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.6}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
      {...props}
    >
      {children}
    </svg>
  );
}

export function ChatIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M4 4.5h12a1 1 0 0 1 1 1v7.5a1 1 0 0 1-1 1H9l-3.5 3v-3H4a1 1 0 0 1-1-1V5.5a1 1 0 0 1 1-1Z" />
    </Icon>
  );
}

export function ClockIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <circle cx="10" cy="10" r="7" />
      <path d="M10 6.2V10l2.6 1.6" />
    </Icon>
  );
}

export function SettingsIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M3.5 6h8M15 6h1.5M3.5 14H5M8.5 14h8" />
      <circle cx="13" cy="6" r="1.8" />
      <circle cx="6.8" cy="14" r="1.8" />
    </Icon>
  );
}

export function PlusIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M10 4.5v11M4.5 10h11" />
    </Icon>
  );
}

export function ArrowUpIcon(props: IconProps) {
  return (
    <Icon {...props} strokeWidth={2}>
      <path d="M10 15.5v-11M5.5 9 10 4.5 14.5 9" />
    </Icon>
  );
}

export function StopIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <rect x="6" y="6" width="8" height="8" rx="1.5" fill="currentColor" stroke="none" />
    </Icon>
  );
}

export function CopyIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <rect x="7" y="7" width="9" height="9" rx="1.5" />
      <path d="M13 4.5H5.5a1 1 0 0 0-1 1V13" />
    </Icon>
  );
}

export function CheckIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="m4.5 10.5 3.5 3.5 7.5-8" />
    </Icon>
  );
}

export function RetryIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M15.5 10a5.5 5.5 0 1 1-1.6-3.9" />
      <path d="M15.5 4v3.2h-3.2" />
    </Icon>
  );
}

export function MoreIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <circle cx="5" cy="10" r="0.6" fill="currentColor" />
      <circle cx="10" cy="10" r="0.6" fill="currentColor" />
      <circle cx="15" cy="10" r="0.6" fill="currentColor" />
    </Icon>
  );
}

export function ChevronDownIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="m6 8 4 4 4-4" />
    </Icon>
  );
}

export function ChevronLeftIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="m12 5-5 5 5 5" />
    </Icon>
  );
}

export function ChevronRightIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="m8 5 5 5-5 5" />
    </Icon>
  );
}

export function TrashIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M4.5 6h11M8 6V4.5h4V6M6 6l.7 9.5h6.6L14 6" />
    </Icon>
  );
}

export function ChevronUpIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="m6 12 4-4 4 4" />
    </Icon>
  );
}

export function ProfileIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <circle cx="10" cy="7" r="3" />
      <path d="M4.5 16.5c.8-2.8 3-4.3 5.5-4.3s4.7 1.5 5.5 4.3" />
    </Icon>
  );
}

export function GlobeIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <circle cx="10" cy="10" r="7" />
      <path d="M3 10h14M10 3c2 2.2 2.8 4.5 2.8 7s-.8 4.8-2.8 7c-2-2.2-2.8-4.5-2.8-7S8 5.2 10 3Z" />
    </Icon>
  );
}

export function ArrowLeftIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M15.5 10h-11M9 5.5 4.5 10 9 14.5" />
    </Icon>
  );
}

export function ArrowRightIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M4.5 10h11M11 5.5l4.5 4.5-4.5 4.5" />
    </Icon>
  );
}

export function ReloadIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M15.5 10a5.5 5.5 0 1 1-1.6-3.9" />
      <path d="M15.5 4v3h-3" />
    </Icon>
  );
}

export function CloseIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="m5.5 5.5 9 9M14.5 5.5l-9 9" />
    </Icon>
  );
}

export function ExternalIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M11 4.5h4.5V9M15.5 4.5 9 11M13.5 12v3a1 1 0 0 1-1 1h-7a1 1 0 0 1-1-1V8a1 1 0 0 1 1-1h3" />
    </Icon>
  );
}

export function MaximizeIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M11.5 4.5h4v4M8.5 15.5h-4v-4M15.5 4.5 11 9M4.5 15.5 9 11" />
    </Icon>
  );
}

export function RestoreIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M15.5 8.5h-4v-4M4.5 11.5h4v4M11.5 8.5 16 4M8.5 11.5 4 16" />
    </Icon>
  );
}

/** A panel with its edge on the given side (dock / collapse controls). */
export function PanelIcon({ side = 'right', ...props }: IconProps & { side?: 'left' | 'right' }) {
  return (
    <Icon {...props}>
      <rect x="3.5" y="4.5" width="13" height="11" rx="1.5" />
      <path d={side === 'right' ? 'M12 4.5v11' : 'M8 4.5v11'} />
    </Icon>
  );
}

export function SparkleIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M10 3.5c.5 3 1.9 4.6 5 5-3.1.5-4.5 2-5 5-.5-3-1.9-4.5-5-5 3.1-.4 4.5-2 5-5ZM15 13.5c.2 1.2.8 1.8 2 2-1.2.2-1.8.8-2 2-.2-1.2-.8-1.8-2-2 1.2-.2 1.8-.8 2-2Z" />
    </Icon>
  );
}

export function FileIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M11.5 3.5H6a1 1 0 0 0-1 1v11a1 1 0 0 0 1 1h8a1 1 0 0 0 1-1V7l-3.5-3.5Z" />
      <path d="M11.5 3.5V7H15" />
    </Icon>
  );
}

export function UploadIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M10 13V4.5M6.5 8 10 4.5 13.5 8M4.5 13.5v2h11v-2" />
    </Icon>
  );
}

/** Analytics: three bars on a baseline. */
export function ChartIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M4 16h12M6.5 13V9.5M10 13V5.5M13.5 13v-5" />
    </Icon>
  );
}

/** Minimize to an indicator: a bar at the bottom. */
export function MinimizeIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M5 14.5h10" />
    </Icon>
  );
}

export function FilterIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M3.5 5h13M6 10h8M8.5 15h3" />
    </Icon>
  );
}

export function SortIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M6.5 4v12M4 13.5 6.5 16 9 13.5M13.5 16V4M11 6.5 13.5 4 16 6.5" />
    </Icon>
  );
}

export function DatabaseIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <ellipse cx="10" cy="5.5" rx="5.5" ry="2" />
      <path d="M4.5 5.5v9c0 1.1 2.5 2 5.5 2s5.5-.9 5.5-2v-9M4.5 10c0 1.1 2.5 2 5.5 2s5.5-.9 5.5-2" />
    </Icon>
  );
}

export function SearchIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <circle cx="9" cy="9" r="5" />
      <path d="m12.8 12.8 3.7 3.7" />
    </Icon>
  );
}

export function BriefcaseIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <rect x="3" y="6.5" width="14" height="10" rx="1.5" />
      <path d="M7.5 6.5V5a1 1 0 0 1 1-1h3a1 1 0 0 1 1 1v1.5M3 11h14" />
    </Icon>
  );
}

export function LinkIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M8.5 11.5a3 3 0 0 0 4.2 0l2.5-2.5a3 3 0 0 0-4.2-4.2l-.8.8M11.5 8.5a3 3 0 0 0-4.2 0L4.8 11a3 3 0 0 0 4.2 4.2l.8-.8" />
    </Icon>
  );
}

export function LanguageIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M3.5 5.5h7M7 4v1.5c0 3-1.6 5.6-3.5 7M5 8.5c1 1.6 2.5 3 4 3.8M10.5 16.5l3-7 3 7M11.5 14.5h4" />
    </Icon>
  );
}

export function SchoolIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M2.5 8 10 4.5 17.5 8 10 11.5 2.5 8ZM5.5 9.5v3.5c1.2 1.3 2.8 2 4.5 2s3.3-.7 4.5-2V9.5M17.5 8v4" />
    </Icon>
  );
}

export function TagIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M3.5 4.5v5l6.8 6.8a1 1 0 0 0 1.4 0l4.6-4.6a1 1 0 0 0 0-1.4L9.5 3.5h-5a1 1 0 0 0-1 1Z" />
      <circle cx="7" cy="7" r="1" />
    </Icon>
  );
}

export function KeyIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <circle cx="7" cy="12.5" r="3.5" />
      <path d="m9.5 10 6-6M13.5 6l1.8 1.8M12 7.5l1.3 1.3" />
    </Icon>
  );
}

export function InfoIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <circle cx="10" cy="10" r="7" />
      <path d="M10 9v4.5M10 6.5v.01" />
    </Icon>
  );
}

export function SunIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <circle cx="10" cy="10" r="3.4" />
      <path d="M10 2.6v1.6M10 15.8v1.6M2.6 10h1.6M15.8 10h1.6M4.77 4.77l1.13 1.13M14.1 14.1l1.13 1.13M4.77 15.23l1.13-1.13M14.1 5.9l1.13-1.13" />
    </Icon>
  );
}

export function MoonIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M16.2 12.3A6.6 6.6 0 0 1 7.7 3.8a6.6 6.6 0 1 0 8.5 8.5Z" />
    </Icon>
  );
}

/** Agents: a small bot face. */
export function AgentIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <rect x="4" y="6.5" width="12" height="9" rx="2.5" />
      <path d="M10 6.5V4M8 10.5v.5M12 10.5v.5M8.2 13.2h3.6" />
      <circle cx="10" cy="3.5" r=".6" />
    </Icon>
  );
}

/** MCP servers: a plug. */
export function PlugIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M7.5 3.5v3M12.5 3.5v3M5.5 6.5h9v2.5a4.5 4.5 0 0 1-9 0V6.5ZM10 13.5v3" />
    </Icon>
  );
}

export function QuestionIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <circle cx="10" cy="10" r="7" />
      <path d="M8 8.2a2 2 0 1 1 2.8 1.8c-.5.3-.8.7-.8 1.3v.3M10 13.8v.01" />
    </Icon>
  );
}

export function EyeIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M2.5 10s2.7-5 7.5-5 7.5 5 7.5 5-2.7 5-7.5 5-7.5-5-7.5-5Z" />
      <circle cx="10" cy="10" r="2.2" />
    </Icon>
  );
}

export function EyeOffIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M4.2 6.6C3 7.9 2.5 10 2.5 10s2.7 5 7.5 5c1.4 0 2.6-.4 3.6-1M8 5.3c.6-.2 1.3-.3 2-.3 4.8 0 7.5 5 7.5 5s-.5 1-1.5 2.2M3.5 3.5l13 13" />
    </Icon>
  );
}

export function StarIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="m10 3.5 2 4.2 4.5.6-3.3 3.1.8 4.5-4-2.2-4 2.2.8-4.5L3.5 8.3 8 7.7l2-4.2Z" />
    </Icon>
  );
}

export function DownloadIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M10 4v8.5M6.5 9 10 12.5 13.5 9M4.5 13.5v2h11v-2" />
    </Icon>
  );
}

export function PencilIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M12.5 4.5 15.5 7.5 7.5 15.5H4.5v-3l8-8ZM11 6l3 3" />
    </Icon>
  );
}

/** Credentials: a rosette. */
export function AwardIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <circle cx="10" cy="8" r="4.5" />
      <path d="m7.3 11.6-1.3 5 4-1.8 4 1.8-1.3-5" />
    </Icon>
  );
}

/** A tool call: a wrench. */
export function ToolIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M12.8 3.6a3.8 3.8 0 0 0-4.5 4.9l-4.6 4.6a1.4 1.4 0 0 0 2 2l4.6-4.6a3.8 3.8 0 0 0 4.9-4.5l-2.3 2.3-2-.4-.4-2 2.3-2.3Z" />
    </Icon>
  );
}

export function LayoutIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <rect x="3.5" y="3.5" width="13" height="13" rx="1.5" />
      <path d="M3.5 7.5h13M8 7.5v9" />
    </Icon>
  );
}

export function TargetIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <circle cx="10" cy="10" r="6.5" />
      <circle cx="10" cy="10" r="3.5" />
      <circle cx="10" cy="10" r=".6" />
    </Icon>
  );
}

export function CompassIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <circle cx="10" cy="10" r="7" />
      <path d="m12.8 7.2-1.6 4-4 1.6 1.6-4 4-1.6Z" />
    </Icon>
  );
}

export function BookIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M4 4.5h4.5A1.5 1.5 0 0 1 10 6v10a1.5 1.5 0 0 0-1.5-1.5H4v-10ZM16 4.5h-4.5A1.5 1.5 0 0 0 10 6v10a1.5 1.5 0 0 1 1.5-1.5H16v-10Z" />
    </Icon>
  );
}

export function CodeIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M7 6.5 3.5 10 7 13.5M13 6.5l3.5 3.5-3.5 3.5M11 4.5l-2 11" />
    </Icon>
  );
}

/** Two overlapping circles: comparing a job with the Profile. */
export function MatchIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <circle cx="7.8" cy="10" r="4.3" />
      <circle cx="12.2" cy="10" r="4.3" />
    </Icon>
  );
}

export function MicIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <rect x="7.5" y="3" width="5" height="9" rx="2.5" />
      <path d="M5 9.5a5 5 0 0 0 10 0M10 14.5V17" />
    </Icon>
  );
}

export function FlagIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M5 17V3.5M5 4h9l-2 3 2 3H5" />
    </Icon>
  );
}

export function ZoomInIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <circle cx="9" cy="9" r="5" />
      <path d="m12.8 12.8 3.7 3.7M7 9h4M9 7v4" />
    </Icon>
  );
}

export function ZoomOutIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <circle cx="9" cy="9" r="5" />
      <path d="m12.8 12.8 3.7 3.7M7 9h4" />
    </Icon>
  );
}
