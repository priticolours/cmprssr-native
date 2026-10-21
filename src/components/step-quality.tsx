import { Sparkles, ShieldCheck, Zap } from "lucide-react";
import { Button, Callout, Text } from "./ui";
import type { CompressionMode } from "../types";
import { QualityCard } from "./quality-card";

interface Props {
  mode: CompressionMode;
  runError: string | null;
  onSelect: (mode: CompressionMode) => void;
  onBack: () => void;
  onStart: () => void;
}

export function StepQuality({ mode, runError, onSelect, onBack, onStart }: Props) {
  return (
    <div className="flex w-full max-w-lg flex-col items-center gap-6 animate-soft-rise">
      <div className="flex flex-col items-center gap-2 text-center">
        <Text variant="heading1">Pick your quality</Text>
        <Text variant="regular" color="tertiary">
          Every option keeps your raw grading controls in DaVinci Resolve.
        </Text>
      </div>

      <div className="flex w-full flex-col gap-2.5">
        <QualityCard
          icon={Sparkles}
          title="Default"
          badge="Recommended"
          description="Great quality, much smaller files."
          detail="No quality loss, with the widest support across editing apps."
          selected={mode === "default"}
          onSelect={() => onSelect("default")}
        />
        <QualityCard
          icon={ShieldCheck}
          title="Lossless"
          description="Squeezed as small as lossless goes."
          detail="Zero quality loss — perfect for archiving your masters."
          selected={mode === "lossless"}
          onSelect={() => onSelect("lossless")}
        />
        <QualityCard
          icon={Zap}
          title="High Compression"
          description="The smallest files, tiny quality trade-off."
          detail="Best for proxies and oversampled footage. Still fully raw in Resolve."
          selected={mode === "high"}
          onSelect={() => onSelect("high")}
        />
      </div>

      {runError ? (
        <Callout color="red" className="w-full">
          {runError}
        </Callout>
      ) : null}

      <div className="flex items-center gap-2">
        <Button variant="transparent" size="large" onClick={onBack}>
          Back
        </Button>
        <Button variant="accent" size="large" onClick={onStart} className="min-w-40">
          Start compressing
        </Button>
      </div>
    </div>
  );
}
