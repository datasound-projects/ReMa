import { SparkleIcon, type IconProps } from '../icons';
import { AGENT_ICONS } from './agentIcons';

export function AgentIcon({ icon, ...props }: { icon: string } & IconProps) {
  const Icon = AGENT_ICONS.find((i) => i.id === icon)?.icon ?? SparkleIcon;
  return <Icon {...props} />;
}
