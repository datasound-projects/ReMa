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
  ModelCatalog,
  ModelOption,
  ModelRef,
  ProviderKind,
  ProviderModel,
  ProviderSettings,
  ProviderView,
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
