import { convertFileSrc } from '@tauri-apps/api/core';

import {
  commands,
  type AddDocumentsResult,
  type CredentialInput,
  type DocumentBlock,
  type DocumentKind,
  type Profile,
  type ProfileCredential,
  type ProfileDocument,
  type ProfileImport,
  type ProfileView,
} from '../generated/bindings';
import { callBackend } from './ipc';

export type {
  AddDocumentsResult,
  CredentialInput,
  CredentialKind,
  CustomField,
  CustomFieldKind,
  DocumentBlock,
  DocumentFormat,
  DocumentKind,
  Education,
  Experience,
  Language,
  Profile,
  ProfileCredential,
  ProfileDocument,
  ProfileImport,
  ProfileLink,
  ProfileView,
  RejectedFile,
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

/**
 * Lets the user pick several files (system dialog in Rust) and stores each.
 * Files that cannot be added are listed with the reason; nothing else
 * changes (no extraction into the Custom Profile).
 */
export function addProfileDocuments(kind: DocumentKind | null): Promise<AddDocumentsResult> {
  return callBackend(() => commands.addProfileDocuments(kind));
}

/** Replaces a document's file with one the user picks; `null` if cancelled. */
export function replaceProfileDocument(id: number): Promise<ProfileDocument | null> {
  return callBackend(() => commands.replaceProfileDocument(id));
}

export function setPrimaryDocument(id: number): Promise<ProfileDocument> {
  return callBackend(() => commands.setPrimaryDocument(id));
}

/** A Word, text or Markdown document as plain blocks (never HTML). */
export function profileDocumentBlocks(id: number): Promise<DocumentBlock[]> {
  return callBackend(() => commands.profileDocumentBlocks(id));
}

/**
 * The stored file's bytes, served to ReMa's own window only by the
 * `rema-doc` protocol (no file paths reach the interface).
 */
export function documentUrl(id: number): string {
  return `${convertFileSrc('document', 'rema-doc')}/${id}`;
}

/** Creates (`id` = null) or updates a credential. */
export function saveCredential(
  id: number | null,
  input: CredentialInput,
  documentId: number | null,
): Promise<ProfileCredential> {
  return callBackend(() => commands.saveCredential(id, input, documentId));
}

/** Deletes a credential and its file. */
export function deleteCredential(id: number): Promise<null> {
  return callBackend(() => commands.deleteCredential(id));
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
