import { FolderInput, HardDrive } from "lucide-react";
import { Button, Callout, Text } from "./ui";

interface Props {
  destPath: string | null;
  sourcePath: string | null;
  onChoose: () => void;
  onBack: () => void;
  onContinue: () => void;
}

function basename(p: string): string {
  return p.split("/").filter(Boolean).pop() ?? p;
}

export function StepDestination({ destPath, sourcePath, onChoose, onBack, onContinue }: Props) {
  const sameAsSource = !!destPath && !!sourcePath && destPath === sourcePath;

  return (
    <div className="flex w-full max-w-lg flex-col items-center gap-6 animate-soft-rise">
      <div className="flex flex-col items-center gap-2 text-center">
        <Text variant="heading1">Where should we save it?</Text>
        <Text variant="regular" color="tertiary">
          Pick a spot for your compressed footage. Your originals stay untouched.
        </Text>
      </div>

      {!destPath ? (
        <Button variant="accent" size="large" onClick={onChoose}>
          <FolderInput className="size-5" />
          Choose destination folder
        </Button>
      ) : (
        <div className="flex w-full items-center gap-3 rounded-card bg-well p-4">
          <div className="flex size-9 shrink-0 items-center justify-center rounded-control bg-control-active text-secondary">
            <HardDrive className="size-5" />
          </div>
          <div className="flex min-w-0 flex-col">
            <Text variant="strong" truncate>
              {basename(destPath)}
            </Text>
            <Text variant="small" color="quaternary" truncate>
              {destPath}
            </Text>
          </div>
          <Button variant="transparent" size="small" className="ml-auto shrink-0" onClick={onChoose}>
            Change
          </Button>
        </div>
      )}

      {sameAsSource ? (
        <Callout color="orange" className="w-full">
          That&rsquo;s the same folder as your footage &mdash; it would overwrite your originals. Pick a different
          folder to be safe.
        </Callout>
      ) : null}

      <div className="flex items-center gap-2">
        <Button variant="transparent" size="large" onClick={onBack}>
          Back
        </Button>
        {destPath ? (
          <Button variant="accent" size="large" onClick={onContinue} disabled={sameAsSource} className="min-w-40">
            Continue
          </Button>
        ) : null}
      </div>
    </div>
  );
}
