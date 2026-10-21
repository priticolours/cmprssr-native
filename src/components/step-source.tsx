import { FolderOpen, Film, Loader2 } from "lucide-react";
import { Badge, Button, Callout, Text } from "./ui";
import type { ScanResult } from "../types";
import { formatBytes, formatCount } from "../format";

interface Props {
  sourcePath: string | null;
  scan: ScanResult | null;
  scanning: boolean;
  scanError: string | null;
  onChoose: () => void;
  onContinue: () => void;
}

function basename(p: string): string {
  return p.split("/").filter(Boolean).pop() ?? p;
}

export function StepSource({ sourcePath, scan, scanning, scanError, onChoose, onContinue }: Props) {
  const hasFrames = !!scan && scan.totalFiles > 0;
  const emptyFolder = !!scan && scan.totalFiles === 0 && !scanning;

  return (
    <div className="flex w-full max-w-lg flex-col items-center gap-6 animate-soft-rise">
      <div className="flex flex-col items-center gap-2 text-center">
        <Text variant="heading1">Where&rsquo;s your footage?</Text>
        <Text variant="regular" color="tertiary">
          Point us to the folder with your CinemaDNG frames &mdash; we&rsquo;ll take a quick look inside.
        </Text>
      </div>

      {!sourcePath ? (
        <Button variant="accent" size="large" onClick={onChoose}>
          <FolderOpen className="size-5" />
          Choose footage folder
        </Button>
      ) : (
        <div className="flex w-full flex-col gap-3 rounded-card bg-well p-4">
          <div className="flex min-w-0 items-center gap-3">
            <div className="flex size-9 shrink-0 items-center justify-center rounded-control bg-control-active text-secondary">
              <Film className="size-5" />
            </div>
            <div className="flex min-w-0 flex-col">
              <Text variant="strong" truncate>
                {basename(sourcePath)}
              </Text>
              <Text variant="small" color="quaternary" truncate>
                {sourcePath}
              </Text>
            </div>
            <Button variant="transparent" size="small" className="ml-auto shrink-0" onClick={onChoose}>
              Change
            </Button>
          </div>

          {scanning ? (
            <div className="flex items-center gap-2">
              <Loader2 className="size-4 animate-spin text-tertiary" />
              <Text variant="small" color="tertiary">
                Scanning footage&hellip;
              </Text>
            </div>
          ) : hasFrames ? (
            <div className="flex flex-wrap gap-2">
              <Badge color="secondary">{formatCount(scan.totalFiles)} frames</Badge>
              <Badge color="secondary">{formatBytes(scan.totalBytes)}</Badge>
              <Badge color="secondary">
                {formatCount(scan.folderCount)} clip{scan.folderCount === 1 ? "" : "s"}
              </Badge>
            </div>
          ) : null}
        </div>
      )}

      {emptyFolder ? (
        <Callout color="orange" className="w-full">
          No CinemaDNG (.dng) frames found in there. Pick the folder that holds your footage.
        </Callout>
      ) : null}
      {scanError ? (
        <Callout color="red" className="w-full">
          {scanError}
        </Callout>
      ) : null}

      {sourcePath ? (
        <Button
          variant="accent"
          size="large"
          onClick={onContinue}
          disabled={!hasFrames || scanning}
          className="w-full max-w-xs"
        >
          Continue
        </Button>
      ) : null}
    </div>
  );
}
