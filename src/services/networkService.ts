import {
  commands,
  type ContactSource,
  type ContactsSummary,
  type NetworkResearchInput,
  type NetworkResult,
} from '../generated/bindings';
import { callBackend } from './ipc';

export type {
  CapabilityItem,
  Company,
  ContactSource,
  ContactsSummary,
  Confidence,
  Connection,
  ConnectionsOutcome,
  Criteria,
  Evidence,
  JobRef,
  NetworkCapability,
  NetworkResearchInput,
  NetworkResult,
  Person,
  ProviderAccess,
  ProviderCapabilities,
  RelevanceType,
  ResultStatus,
  Row,
  Stage,
  StageReport,
  Supports,
} from '../generated/bindings';

/** What LinkedIn and XING let ReMa do right now (never tokens). */
export function getNetworkCapabilities() {
  return callBackend(() => commands.networkCapabilities());
}

/** Researches companies, jobs, people and permitted connections. */
export function researchNetwork(input: NetworkResearchInput): Promise<NetworkResult> {
  return callBackend(() => commands.networkResearch(input));
}

export function cancelNetworkResearch(runId: string): Promise<boolean> {
  return callBackend(() => commands.networkCancel(runId));
}

/** The last result this session (kept in memory only). */
export async function getLastNetworkResult(): Promise<NetworkResult | null> {
  const last = await callBackend(() => commands.networkLastResult());
  return last.result;
}

/** Counts of the contacts the user imported (LinkedIn export, vCards). */
export function getNetworkContacts(): Promise<ContactsSummary> {
  return callBackend(() => commands.networkContacts());
}

/**
 * Opens a file picker for the LinkedIn data export (ZIP or Connections.csv)
 * or vCard files and imports them; null if the user cancelled.
 */
export function importNetworkContacts(source: ContactSource) {
  return callBackend(() => commands.importNetworkContacts(source));
}

/** Removes imported contacts: one source, or all (`null`). */
export function clearNetworkContacts(source: ContactSource | null): Promise<ContactsSummary> {
  return callBackend(() => commands.clearNetworkContacts(source));
}
