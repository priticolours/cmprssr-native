// Shared types for the compression flow. Mirrors main/services/dng-converter.ts.

export type CompressionMode = "default" | "lossless" | "high";

export interface EngineStatus {
  installed: boolean;
  path: string | null;
}

export interface ScanResult {
  totalFiles: number;
  totalBytes: number;
  folderCount: number;
  /** Full paths of up to 8 DNG files, for the film-gate idle preview. */
  samplePaths: string[];
}

export interface CompressParams {
  sourceDir: string;
  destDir: string;
  mode: CompressionMode;
  recursive: boolean;
  /** 0 = auto (half the cores, min 2), 1–8 explicit. */
  workers?: number;
}

export interface ProgressChunk {
  filesDone: number;
  totalFiles: number;
  currentFile: string;
  /** Full path of the file being worked on (for the film-gate thumbnail). */
  currentPath: string;
  bytesIn: number;
  bytesOut: number;
  failed: number;
  filesSkipped: number;
}

export interface JobResult {
  filesProcessed: number;
  filesFailed: number;
  filesCopied: number;
  filesSkipped: number;
  bytesIn: number;
  bytesOut: number;
  ratio: number;
  imageBytesIn: number;
  imageBytesOut: number;
  trueRatio: number;
  elapsedMs: number;
  outputDir: string;
  canceled: boolean;
}

export interface InstallResult {
  success: boolean;
  error?: string;
}
