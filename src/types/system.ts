/** Mirrors `BackendStatus` in `src-tauri/src/models/system.rs`. */
export type BackendStatus = 'ready';

/** Mirrors `AppStatus` in `src-tauri/src/models/system.rs`. */
export interface AppStatus {
  status: BackendStatus;
  app: string;
  version: string;
}
