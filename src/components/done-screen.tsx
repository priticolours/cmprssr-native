import { CheckCircle2, FolderOpen, RotateCcw, XCircle } from "lucide-react";
import { Badge, Button, Callout, Text } from "./ui";
import type { JobResult } from "../types";
import { formatBytes, formatDuration, formatCount, savedPercent } from "../format";

interface Props {
  result: JobResult;
  onReveal: () => void;
  onRestart: () => void;
}

export function DoneScreen({ result, onReveal, onRestart }: Props) {
  const saved = savedPercent(result.bytesIn, result.bytesOut);
  const canceled = result.canceled;

  return (
    <div className="flex w-full max-w-md flex-col items-center gap-6 animate-soft-rise">
      <div
        className={
          canceled
            ? "flex size-14 items-center justify-center rounded-panel bg-control-active text-tertiary"
            : "flex size-14 items-center justify-center rounded-panel bg-accent/10 text-accent"
        }
      >
        {canceled ? <XCircle className="size-7" /> : <CheckCircle2 className="size-7" />}
      </div>

      <div className="flex flex-col items-center gap-2 text-center">
        <Text variant="heading1">{canceled ? "Job canceled" : "Done — your footage is smaller"}</Text>
        <Text variant="regular" color="tertiary">
          {canceled
            ? "We stopped safely. The frames finished before you canceled are saved."
            : "Drag the compressed folder into DaVinci Resolve — all your raw controls are intact."}
        </Text>
      </div>

      <div className="flex w-full flex-col items-center gap-3 rounded-card bg-well px-5 py-5">
        <Text variant="heading1" color={canceled ? "primary" : "accent"} className="tabular-nums">
          {saved > 0 ? `Saved ${saved}%` : "No change"}
        </Text>
        {result.bytesIn > 0 ? (
          <Text variant="regular" color="secondary" className="tabular-nums">
            {formatBytes(result.bytesIn)} &rarr; {formatBytes(result.bytesOut)}
          </Text>
        ) : null}
        <div className="flex flex-wrap justify-center gap-2">
          {saved > 0 ? <Badge color="green">{result.ratio.toFixed(1)}:1</Badge> : null}
          <Badge color="secondary">{formatCount(result.filesProcessed)} frames</Badge>
          <Badge color="secondary">{formatDuration(result.elapsedMs)}</Badge>
        </div>
      </div>

      {result.filesFailed > 0 ? (
        <Callout color="yellow" className="w-full">
          {formatCount(result.filesFailed)} frame{result.filesFailed === 1 ? "" : "s"} couldn&rsquo;t be compressed and
          {result.filesFailed === 1 ? " was" : " were"} copied through unchanged.
        </Callout>
      ) : null}

      <div className="flex items-center gap-2">
        <Button variant="accent" size="large" onClick={onReveal}>
          <FolderOpen className="size-5" />
          Reveal in Finder
        </Button>
        <Button variant="transparent" size="large" onClick={onRestart}>
          <RotateCcw className="size-5" />
          Compress more
        </Button>
      </div>
    </div>
  );
}
