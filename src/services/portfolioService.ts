import {
  commands,
  type AiProposal,
  type AiRequest,
  type PortfolioCreate,
  type PortfolioDocument,
  type PortfolioImport,
  type PortfolioInput,
  type PortfolioKind,
} from '../generated/bindings';
import { callBackend } from './ipc';

export type {
  AiAction,
  AiProposal,
  AiRequest,
  AiScope,
  AiSource,
  CoverLetter,
  PageSize,
  PortfolioContent,
  PortfolioCreate,
  PortfolioDocument,
  PortfolioEntry,
  PortfolioHeader,
  PortfolioImport,
  PortfolioInput,
  PortfolioKind,
  PortfolioSection,
  PortfolioStart,
  PortfolioStyle,
  SectionKind,
} from '../generated/bindings';

export function listPortfolios(): Promise<PortfolioDocument[]> {
  return callBackend(() => commands.listPortfolios());
}

export function getPortfolio(id: number): Promise<PortfolioDocument> {
  return callBackend(() => commands.getPortfolio(id));
}

/** A new document: blank, sample, a copy of the Custom Profile, or reviewed import content. */
export function createPortfolio(request: PortfolioCreate): Promise<PortfolioDocument> {
  return callBackend(() => commands.createPortfolio(request));
}

export function savePortfolio(id: number, input: PortfolioInput): Promise<PortfolioDocument> {
  return callBackend(() => commands.savePortfolio(id, input));
}

export function renamePortfolio(id: number, name: string): Promise<PortfolioDocument> {
  return callBackend(() => commands.renamePortfolio(id, name));
}

export function duplicatePortfolio(id: number): Promise<PortfolioDocument> {
  return callBackend(() => commands.duplicatePortfolio(id));
}

export function deletePortfolio(id: number): Promise<null> {
  return callBackend(() => commands.deletePortfolio(id));
}

/** Picks a file (system dialog in Rust) and reads it for review; `null` if cancelled. */
export function importPortfolioFile(kind: PortfolioKind): Promise<PortfolioImport | null> {
  return callBackend(() => commands.importPortfolioFile(kind));
}

/** Reads a document already in the Profile for review. */
export function importPortfolioDocument(documentId: number, kind: PortfolioKind): Promise<PortfolioImport> {
  return callBackend(() => commands.importPortfolioDocument(documentId, kind));
}

/** One AI assistant request; the answer is a proposal for review. */
export function portfolioAiAssist(request: AiRequest): Promise<AiProposal> {
  return callBackend(() => commands.portfolioAiAssist(request));
}

/**
 * Saves the rendered PDF where the user chooses (system dialog in Rust).
 * Resolves to the file name, or `null` if the user cancelled.
 */
export function exportPortfolioPdf(id: number, pdfBase64: string): Promise<string | null> {
  return callBackend(() => commands.exportPortfolioPdf(id, pdfBase64));
}

export function toInput(document: PortfolioDocument): PortfolioInput {
  return {
    name: document.name,
    kind: document.kind ?? 'cv',
    templateId: document.templateId,
    pageSize: document.pageSize,
    accent: document.accent,
    style: document.style ?? {},
    content: document.content,
    letter: document.letter ?? emptyLetter(),
    sourceDocumentId: document.sourceDocumentId ?? null,
  };
}

export function emptyLetter() {
  return {
    recipientName: '',
    recipientTitle: '',
    company: '',
    address: '',
    position: '',
    date: '',
    subject: '',
    greeting: '',
    body: '',
    closing: '',
    signature: '',
  };
}
