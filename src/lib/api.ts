// Thin, typed wrappers over the Tauri command surface for this app's channels.
// Same `api` object shape as the original; implemented with invoke/listen.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  CompressParams,
  EngineStatus,
  InstallResult,
  JobResult,
  ProgressChunk,
  ScanResult,
} from "../types";

/**
 * Tauri's invoke rejects with the raw backend string, not an Error. Wrap it so
 * the UI's `err instanceof Error ? err.message : fallback` checks keep working
 * with the real backend messages.
 */
async function invokeChecked<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(cmd, args);
  } catch (err) {
    if (err instanceof Error) throw err;
    throw new Error(typeof err === "string" && err.length > 0 ? err : "Unexpected error");
  }
}

export const api = {
  checkEngine: (): Promise<EngineStatus> => invokeChecked("check_engine"),

  installEngine: (): Promise<InstallResult> => invokeChecked("install_engine"),

  openDownloadPage: (): Promise<void> => invokeChecked("open_download_page"),

  scan: (sourceDir: string, recursive: boolean): Promise<ScanResult> =>
    invokeChecked("scan_folder", { sourceDir, recursive }),

  reveal: (path: string): Promise<void> => invokeChecked("reveal", { path }),

  chooseFolder: async (title: string): Promise<string | null> => {
    const p = await invokeChecked<string | null>("pick_folder", { title });
    return p;
  },

  runCompression: async (
    params: CompressParams,
    onChunk: (chunk: ProgressChunk) => void,
    cancellationId: string,
  ): Promise<JobResult> => {
    const unlisten = await listen<ProgressChunk>(`compress-progress-${cancellationId}`, (e) =>
      onChunk(e.payload),
    );
    try {
      return await invokeChecked<JobResult>("run_compress", {
        sourceDir: params.sourceDir,
        destDir: params.destDir,
        mode: params.mode,
        recursive: params.recursive,
        workers: params.workers ?? 0,
        jobId: cancellationId,
      });
    } finally {
      unlisten();
    }
  },

  cancel: (cancellationId: string): void => {
    void invokeChecked("cancel_job", { jobId: cancellationId });
  },

  /** Data URL of a DNG's frame preview: embedded JPEG when it has one,
   *  otherwise a tiny grayscale video-tap synthesized from the raw data;
   *  null only when neither is possible. */
  thumbnail: (path: string): Promise<string | null> =>
    invokeChecked("frame_thumbnail", { path }),
};
