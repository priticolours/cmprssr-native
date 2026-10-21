import { Check } from "lucide-react";
import { cn } from "../lib/utils";
import { Text } from "./ui";

const STEPS = ["Footage", "Destination", "Quality"] as const;

/** Compact 1-2-3 progress header for the setup steps. `current` is 1-based. */
export function StepIndicator({ current }: { current: number }) {
  return (
    <div className="flex items-center justify-center gap-2">
      {STEPS.map((label, index) => {
        const stepNumber = index + 1;
        const done = stepNumber < current;
        const active = stepNumber === current;
        return (
          <div key={label} className="flex items-center gap-2">
            <div className="flex items-center gap-2">
              <div
                className={cn(
                  "flex size-6 shrink-0 items-center justify-center rounded-full text-small-strong tabular-nums",
                  done && "bg-accent text-accent-contrast",
                  active && "bg-accent text-accent-contrast",
                  !done && !active && "bg-control-active text-tertiary",
                )}
              >
                {done ? <Check className="size-3.5" /> : stepNumber}
              </div>
              <Text variant="small-strong" color={active || done ? "primary" : "tertiary"}>
                {label}
              </Text>
            </div>
            {stepNumber < STEPS.length ? (
              <div className={cn("h-px w-6", done ? "bg-accent" : "bg-separator")} />
            ) : null}
          </div>
        );
      })}
    </div>
  );
}
