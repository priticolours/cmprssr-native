import { cn } from "../lib/utils";

const SPROCKETS =
  "repeating-linear-gradient(to right, currentColor 0, currentColor 8px, transparent 8px, transparent 20px)";

/**
 * Decorative cinema film-strip. Sprocket rows scroll when `animated` so the
 * progress screen feels like footage moving through a projector.
 */
export function FilmStrip({ animated = false, className }: { animated?: boolean; className?: string }) {
  return (
    <div
      className={cn(
        "relative h-20 w-full max-w-sm overflow-hidden rounded-card bg-control-active text-quaternary/50",
        className,
      )}
    >
      <div
        className={cn("absolute inset-x-0 top-1.5 h-3 [background-size:48px_100%]", animated && "film-strip-run")}
        style={{ backgroundImage: SPROCKETS }}
      />
      <div
        className={cn("absolute inset-x-0 bottom-1.5 h-3 [background-size:48px_100%]", animated && "film-strip-run")}
        style={{ backgroundImage: SPROCKETS }}
      />
      <div className="absolute inset-0 flex items-center justify-center gap-2 px-6">
        {Array.from({ length: 4 }).map((_, i) => (
          <div
            key={i}
            className={cn(
              "h-7 w-11 shrink-0 rounded-sm bg-well",
              animated && i === 1 && "bg-accent/70",
            )}
          />
        ))}
      </div>
    </div>
  );
}
