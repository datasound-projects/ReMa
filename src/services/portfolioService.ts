import {
  commands,
  type PageSize,
  type PortfolioDocument,
  type PortfolioInput,
  type PortfolioStart,
} from '../generated/bindings';
import { callBackend } from './ipc';

export type {
  PageSize,
  PortfolioContent,
  PortfolioDocument,
  PortfolioEntry,
  PortfolioHeader,
  PortfolioInput,
  PortfolioSection,
  PortfolioStart,
  SectionKind,
} from '../generated/bindings';

export function listPortfolios(): Promise<PortfolioDocument[]> {
  return callBackend(() => commands.listPortfolios());
}

/** A new CV: empty sections, or a one-time copy of the Custom Profile. */
export function createPortfolio(
  name: string,
  templateId: string,
  pageSize: PageSize,
  start: PortfolioStart,
): Promise<PortfolioDocument> {
  return callBackend(() => commands.createPortfolio(name, templateId, pageSize, start));
}

export function savePortfolio(id: number, input: PortfolioInput): Promise<PortfolioDocument> {
  return callBackend(() => commands.savePortfolio(id, input));
}

export function duplicatePortfolio(id: number): Promise<PortfolioDocument> {
  return callBackend(() => commands.duplicatePortfolio(id));
}

export function deletePortfolio(id: number): Promise<null> {
  return callBackend(() => commands.deletePortfolio(id));
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
    templateId: document.templateId,
    pageSize: document.pageSize,
    accent: document.accent,
    content: document.content,
  };
}
