import { commands, type AnswerMode, type CareerSearchStatus } from '../generated/bindings';
import { callBackend } from './ipc';

export type {
  AnswerMode,
  CareerSearchStatus,
  CheckResult,
  RouteReport,
  RouteState,
  RuntimeCapabilities,
  SourceHealth,
} from '../generated/bindings';

/** Career search: automatic, its routes and sources. Nothing to set up. */
export function careerSearchStatus(): Promise<CareerSearchStatus> {
  return callBackend(() => commands.careerSearchStatus());
}

/** Checks that ReMa's own sources and company research answer now. */
export function checkCareerSearch(): Promise<CareerSearchStatus> {
  return callBackend(() => commands.checkCareerSearch());
}

/**
 * How chats answer questions that need the web: the model's own search (as
 * in ChatGPT and Claude) or ReMa's verified search.
 */
export function setAnswerMode(mode: AnswerMode): Promise<CareerSearchStatus> {
  return callBackend(() => commands.setAnswerMode(mode));
}
