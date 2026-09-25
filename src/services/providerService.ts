import {
  commands,
  type CustomProviderInput,
  type ModelCatalog,
  type ModelRef,
  type ProviderKind,
  type ProviderSettings,
  type ProviderView,
} from '../generated/bindings';
import { callBackend } from './ipc';

export type {
  ConnectionMethod,
  ConnectionStatus,
  ModelCatalog,
  ModelOption,
  ModelRef,
  ProviderKind,
  ProviderModel,
  ProviderSettings,
  ProviderView,
  SignInStatus,
  SignInView,
} from '../generated/bindings';

export function getProviderSettings(): Promise<ProviderSettings> {
  return callBackend(() => commands.getProviderSettings());
}

export function getModelCatalog(): Promise<ModelCatalog> {
  return callBackend(() => commands.getModelCatalog());
}

export function connectProvider(kind: ProviderKind, apiKey: string): Promise<ProviderView> {
  return callBackend(() => commands.connectProvider(kind, apiKey));
}

/**
 * Starts the account sign-in in the system browser (ChatGPT through OpenAI's
 * Codex app, Claude Console through Anthropic's CLI). Resolves at once;
 * progress arrives with `providersChanged`.
 */
export function startProviderSignIn(kind: ProviderKind, deviceCode = false): Promise<ProviderView> {
  return callBackend(() => commands.startProviderSignIn(kind, deviceCode));
}

/** Cancels a running sign-in, or clears a finished one's notice. */
export function cancelProviderSignIn(kind: ProviderKind): Promise<ProviderView> {
  return callBackend(() => commands.cancelProviderSignIn(kind));
}

/** Asks an account connection's runtime whether the sign-in still works. */
export function checkProviderConnection(providerId: string): Promise<ProviderView> {
  return callBackend(() => commands.checkProviderConnection(providerId));
}

/** Signs out with the runtime's official logout, then disconnects. */
export function signOutProvider(providerId: string): Promise<null> {
  return callBackend(() => commands.signOutProvider(providerId));
}

export function disconnectProvider(providerId: string): Promise<null> {
  return callBackend(() => commands.disconnectProvider(providerId));
}

export function saveCustomProvider(input: CustomProviderInput): Promise<ProviderView> {
  return callBackend(() => commands.saveCustomProvider(input));
}

export function refreshProviderModels(providerId: string): Promise<ProviderView> {
  return callBackend(() => commands.refreshProviderModels(providerId));
}

export function setModelEnabled(model: ModelRef, enabled: boolean): Promise<null> {
  return callBackend(() => commands.setModelEnabled(model, enabled));
}

export function setDefaultModel(model: ModelRef): Promise<null> {
  return callBackend(() => commands.setDefaultModel(model));
}

export function sameModel(a: ModelRef | null | undefined, b: ModelRef | null | undefined): boolean {
  return !!a && !!b && a.providerId === b.providerId && a.modelId === b.modelId;
}
