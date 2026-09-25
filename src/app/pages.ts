import type { ComponentType } from 'react';

import { HomeIcon, type IconProps } from '../components/icons';
import { HomePage } from '../pages/HomePage';

export interface PageDefinition {
  id: string;
  label: string;
  icon: ComponentType<IconProps>;
  component: ComponentType;
}

/**
 * Page registry. Each entry appears in the sidebar and is rendered in the
 * main area when selected — adding a page means adding one entry here.
 */
export const PAGES = [
  { id: 'home', label: 'Home', icon: HomeIcon, component: HomePage },
] as const satisfies readonly PageDefinition[];

export type PageId = (typeof PAGES)[number]['id'];

export const DEFAULT_PAGE_ID: PageId = 'home';
