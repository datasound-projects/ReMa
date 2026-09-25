import {
  commands,
  type AutofillResult,
  type BrowserBounds,
  type BrowserStatus,
} from '../generated/bindings';
import { callBackend } from './ipc';

export type {
  AutofillResult,
  BrowserBounds,
  BrowserStatus,
  FileField,
  FileFieldKind,
} from '../generated/bindings';

export function getBrowserStatus(): Promise<BrowserStatus> {
  return callBackend(() => commands.getBrowserStatus());
}

/** Opens a web page in the isolated browser workspace at `bounds`. */
export function openInBrowser(url: string, bounds: BrowserBounds): Promise<BrowserStatus> {
  return callBackend(() => commands.openInBrowser(url, bounds));
}

export function setBrowserBounds(bounds: BrowserBounds): Promise<null> {
  return callBackend(() => commands.setBrowserBounds(bounds));
}

export function setBrowserVisible(visible: boolean): Promise<null> {
  return callBackend(() => commands.setBrowserVisible(visible));
}

export function browserBack(): Promise<null> {
  return callBackend(() => commands.browserBack());
}

export function browserForward(): Promise<null> {
  return callBackend(() => commands.browserForward());
}

export function browserReload(): Promise<null> {
  return callBackend(() => commands.browserReload());
}

export function closeBrowser(): Promise<null> {
  return callBackend(() => commands.closeBrowser());
}

/** Fills the page's form from the Profile. Never submits. */
export function runAutofill(): Promise<AutofillResult> {
  return callBackend(() => commands.runAutofill());
}

export function attachProfileDocument(field: number, documentId: number): Promise<null> {
  return callBackend(() => commands.attachProfileDocument(field, documentId));
}
