import { commands, type CareerSearchStatus } from '../generated/bindings';
import { callBackend } from './ipc';

export type { CareerSearchStatus, CheckResult, RouteReport, RouteState, SourceHealth } from '../generated/bindings';

/** Career search: automatic, its routes and sources. Nothing to set up. */
export function careerSearchStatus(): Promise<CareerSearchStatus> {
  return callBackend(() => commands.careerSearchStatus());
}

/** Checks that ReMa's own sources and company research answer now. */
export function checkCareerSearch(): Promise<CareerSearchStatus> {
  return callBackend(() => commands.checkCareerSearch());
}
