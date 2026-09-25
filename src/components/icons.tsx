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
