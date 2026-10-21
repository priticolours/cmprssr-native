import type { LucideIcon } from "lucide-react";
import { Check } from "lucide-react";
import { cn } from "../lib/utils";
import { Badge, Text } from "./ui";

export interface QualityCardProps {
  icon: LucideIcon;
  title: string;
  description: string;
  detail: string;
  badge?: string;
  selected: boolean;
  onSelect: () => void;
}

/** Large, tappable quality option card used on the quality step. */
export function QualityCard({ icon: Icon, title, description, detail, badge, selected, onSelect }: QualityCardProps) {
  return (
    <button
      type="button"
      onClick={onSelect}
      aria-pressed={selected}
      className={cn(
        "flex w-full items-start gap-4 rounded-card p-4 text-left transition-colors",
        "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent",
        selected ? "bg-accent/10" : "bg-control-subtle hover:bg-control",
      )}
      style={selected ? { boxShadow: "inset 0 0 0 1.5px var(--color-accent)" } : undefined}
    >
      <div
        className={cn(
          "flex size-10 shrink-0 items-center justify-center rounded-control",
          selected ? "bg-accent text-accent-contrast" : "bg-well text-secondary",
        )}
      >
        <Icon className="size-5" />
      </div>
      <div className="flex min-w-0 flex-col gap-0.5">
        <div className="flex items-center gap-2">
          <Text variant="large-strong">{title}</Text>
          {badge ? <Badge color="blue">{badge}</Badge> : null}
        </div>
        <Text variant="regular" color="secondary">
          {description}
        </Text>
        <Text variant="small" color="tertiary">
          {detail}
        </Text>
      </div>
      <div className="ml-auto shrink-0 self-center">
        {selected ? (
          <div className="flex size-5 items-center justify-center rounded-full bg-accent text-accent-contrast">
            <Check className="size-3.5" />
          </div>
        ) : (
          <div className="size-5 rounded-full border border-separator" />
        )}
      </div>
    </button>
  );
}
