import { commands, type WebSearchInput, type WebSearchSettings, type WebSearchTest } from '../generated/bindings';
import { callBackend } from './ipc';

export type { ServiceKind, WebSearchInput, WebSearchSettings, WebSearchTest } from '../generated/bindings';

/** The search service ReMa uses when a model has no web search of its own. The key never reaches the interface. */
export function webSearchSettings(): Promise<WebSearchSettings> {
  return callBackend(() => commands.webSearchSettings());
}

/** Saves the service; a key goes to the system keychain (`key: null` keeps the stored one). */
export function saveWebSearchSettings(input: WebSearchInput): Promise<WebSearchSettings> {
  return callBackend(() => commands.saveWebSearchSettings(input));
}

/** Runs one real search with the saved service. */
export function testWebSearch(): Promise<WebSearchTest> {
  return callBackend(() => commands.testWebSearch());
}
