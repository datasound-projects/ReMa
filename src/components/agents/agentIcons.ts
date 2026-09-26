import type { ComponentType } from 'react';

import {
  BookIcon,
  ChartIcon,
  ChatIcon,
  CodeIcon,
  CompassIcon,
  FileIcon,
  FlagIcon,
  MatchIcon,
  MicIcon,
  SearchIcon,
  SparkleIcon,
  TargetIcon,
  type IconProps,
} from '../icons';

/** The icons an agent can have (the same list as `AGENT_ICONS` in Rust). */
export const AGENT_ICONS: { id: string; label: string; icon: ComponentType<IconProps> }[] = [
  { id: 'spark', label: 'Spark', icon: SparkleIcon },
  { id: 'search', label: 'Search', icon: SearchIcon },
  { id: 'match', label: 'Match', icon: MatchIcon },
  { id: 'document', label: 'Document', icon: FileIcon },
  { id: 'interview', label: 'Interview', icon: MicIcon },
  { id: 'strategy', label: 'Strategy', icon: FlagIcon },
  { id: 'research', label: 'Research', icon: ChartIcon },
  { id: 'target', label: 'Target', icon: TargetIcon },
  { id: 'compass', label: 'Compass', icon: CompassIcon },
  { id: 'chat', label: 'Chat', icon: ChatIcon },
  { id: 'book', label: 'Book', icon: BookIcon },
  { id: 'code', label: 'Code', icon: CodeIcon },
];

