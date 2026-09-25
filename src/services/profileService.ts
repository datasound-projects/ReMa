import {
  commands,
  type DocumentKind,
  type Profile,
  type ProfileDocument,
  type ProfileImport,
  type ProfileView,
} from '../generated/bindings';
import { callBackend } from './ipc';

export type {
  CustomField,
  CustomFieldKind,
  DocumentFormat,
  DocumentKind,
  Education,
  Experience,
  Language,
  Profile,
  ProfileDocument,
  ProfileImport,
  ProfileLink,
  ProfileView,
} from '../generated/bindings';

export function getProfile(): Promise<ProfileView> {
  return callBackend(() => commands.getProfile());
}

/** Validates and stores the whole profile; returns the saved state. */
export function saveProfile(profile: Profile): Promise<ProfileView> {
  return callBackend(() => commands.saveProfile(profile));
}

/**
 * Opens the system file picker (in Rust) and stores the chosen file.
 * Resolves to `null` if the user cancelled.
 */
export function addProfileDocument(kind: DocumentKind | null): Promise<ProfileDocument | null> {
  return callBackend(() => commands.addProfileDocument(kind));
}

/** Reads profile details from a document for review. Saves nothing. */
export function importProfileDocument(documentId: number): Promise<ProfileImport> {
  return callBackend(() => commands.importProfileDocument(documentId));
}

export function updateProfileDocument(
  id: number,
  name: string,
  kind: DocumentKind,
): Promise<ProfileDocument> {
  return callBackend(() => commands.updateProfileDocument(id, name, kind));
}

export function deleteProfileDocument(id: number): Promise<null> {
  return callBackend(() => commands.deleteProfileDocument(id));
}

export function openProfileDocument(id: number): Promise<null> {
  return callBackend(() => commands.openProfileDocument(id));
}

export function revealProfileDocument(id: number): Promise<null> {
  return callBackend(() => commands.revealProfileDocument(id));
}

export function emptyProfile(): Profile {
  return {
    firstName: '',
    lastName: '',
    email: '',
    phone: '',
    location: '',
    title: '',
    summary: '',
    skills: [],
    experience: [],
    education: [],
    languages: [],
    website: '',
    resumeWebsite: '',
    github: '',
    linkedin: '',
    otherLinks: [],
    customFields: [],
  };
}
