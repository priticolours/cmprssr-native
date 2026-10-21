import * as React from "react";
import { Button, Text } from "./ui";
import { cn } from "../lib/utils";
import type { CompressParams, JobResult, ProgressChunk } from "../types";
import { api } from "../lib/api";
import { formatDuration, savedPercent } from "../format";
import { FilmStrip } from "./film-strip";

interface Props {
  params: CompressParams;
  expectedTotal: number;
  onFinish: (result: JobResult) => void;
  onError: (message: string) => void;
}

function makeId(): string {
  return `job-${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

export function ProgressScreen({ params, expectedTotal, onFinish, onError }: Props) {
  const [progress, setProgress] = React.useState<ProgressChunk | null>(null);
  const [elapsed, setElapsed] = React.useState(0);
  const [canceling, setCanceling] = React.useState(false);
  const cancelIdRef = React.useRef<string>(makeId());

  React.useEffect(() => {
    let active = true;
    const startedAt = Date.now();
    const timer = window.setInterval(() => {
      if (active) setElapsed(Date.now() - startedAt);
    }, 500);

    api
      .runCompression(
        params,
        (chunk) => {
          if (active) setProgress(chunk);
        },
        cancelIdRef.current,
      )
      .then((result) => {
        if (active) onFinish(result);
      })
      .catch((err: unknown) => {
        if (active) onError(err instanceof Error ? err.message : "Something went wrong while compressing.");
      });

    return () => {
      active = false;
      window.clearInterval(timer);
      api.cancel(cancelIdRef.current);
    };
  }, []);

  const total = progress?.totalFiles || expectedTotal || 0;
  const done = progress?.filesDone ?? 0;
  const percent = total > 0 ? Math.min(100, Math.round((done / total) * 100)) : 0;
  const bytesIn = progress?.bytesIn ?? 0;
  const bytesOut = progress?.bytesOut ?? 0;
  const saved = savedPercent(bytesIn, bytesOut);

  const handleCancel = () => {
    setCanceling(true);
    api.cancel(cancelIdRef.current);
  };

  return (
    <div className="flex w-full max-w-md flex-col items-center gap-6">
      <FilmStrip animated={!canceling} />

      <div className="flex flex-col items-center gap-1 text-center">
        <Text variant="heading1">{canceling ? "Wrapping up…" : "Compressing your footage…"}</Text>
        <Text variant="regular" color="tertiary">
          {canceling ? "Finishing the current frame safely." : "Sit back — this can take a while for big shoots."}
        </Text>
      </div>

      <div className="flex w-full flex-col gap-2">
        <div className="flex items-baseline justify-between">
          <Text variant="large-strong" className="tabular-nums">
            {percent}%
          </Text>
          <Text variant="small" color="tertiary" className="tabular-nums">
            {done.toLocaleString()} / {total.toLocaleString()} frames
          </Text>
        </div>
        <div className="h-2 w-full overflow-hidden rounded-pill bg-well">
          <div
            className={cn("h-full rounded-pill bg-accent transition-[width] duration-300 ease-out")}
            style={{ width: `${percent}%` }}
          />
        </div>
      </div>

      <div className="flex w-full items-center justify-between gap-4 rounded-card bg-well px-4 py-3">
        <div className="flex flex-col">
          <Text variant="small" color="quaternary">
            Saved so far
          </Text>
          <Text variant="strong" className="tabular-nums">
            {saved > 0 ? `${saved}%` : "—"}
          </Text>
        </div>
        <div className="flex flex-col items-end">
          <Text variant="small" color="quaternary">
            Elapsed
          </Text>
          <Text variant="strong" className="tabular-nums">
            {formatDuration(elapsed)}
          </Text>
        </div>
      </div>

      {progress?.currentFile ? (
        <Text variant="small-mono" color="quaternary" truncate className="w-full text-center">
          {progress.currentFile}
        </Text>
      ) : null}

      <Button variant="muted" size="medium" onClick={handleCancel} disabled={canceling}>
        {canceling ? "Canceling…" : "Cancel"}
      </Button>
    </div>
  );
}
