import React from "react";

export interface GateFrame {
  key: string;
  thumb: string | null;
  label: string;
}

/** Single-frame film gate: one frame in the chamber at a time. Each new
 *  frame slides in from the right like film stepping through the gate.
 *  Frames are looked up from the live prop by key so async thumbnails
 *  landing after the slide still paint into the visible frame. */
export function FilmGate({
  frames,
  running,
  emptyLabel,
}: {
  frames: GateFrame[];
  running: boolean;
  emptyLabel: string;
}) {
  const [keys, setKeys] = React.useState<string[]>([]);
  const [sliding, setSliding] = React.useState(false);
  const lastKeyRef = React.useRef<string | null>(null);

  const latestKey = frames.length > 0 ? frames[frames.length - 1].key : null;

  React.useEffect(() => {
    if (frames.length === 0) {
      setKeys([]);
      setSliding(false);
      lastKeyRef.current = null;
      return;
    }
    const cur = frames[frames.length - 1];
    if (lastKeyRef.current === cur.key) return;
    lastKeyRef.current = cur.key;
    setKeys((prev) => [...prev.slice(-1), cur.key]);
    setSliding(true);
    const t = window.setTimeout(() => {
      setKeys((prev) => prev.slice(-1));
      setSliding(false);
    }, 220);
    return () => window.clearTimeout(t);
  }, [latestKey]); // eslint-disable-line react-hooks/exhaustive-deps

  const byKey = React.useMemo(() => new Map(frames.map((f) => [f.key, f])), [frames]);
  const pair = keys
    .map((k) => byKey.get(k))
    .filter((f): f is GateFrame => Boolean(f));
  const current = pair.length > 0 ? pair[pair.length - 1] : null;

  return (
    <div className="gate-single">
      <div className="faceplate-micro-label gate-label">
        GATE{running ? " \u2014 RUNNING" : ""}
      </div>
      <div className="gate-chamber">
        <div className="gate-sprockets" aria-hidden="true" />
        <div className="gate-viewport">
          {current ? (
            <div
              className="gate-slide"
              style={{
                transform:
                  pair.length > 1 && sliding ? "translateX(-50%)" : "translateX(0)",
                transition: sliding ? "transform 200ms ease-out" : "none",
              }}
            >
              {pair.map((f) => (
                <div className="gate-frame" key={f.key}>
                  {f.thumb ? (
                    <img src={f.thumb} alt={f.label} draggable={false} />
                  ) : (
                    <span className="gate-frame-name">{f.label}</span>
                  )}
                </div>
              ))}
            </div>
          ) : (
            <div className="gate-empty">{emptyLabel}</div>
          )}
          {current ? <div className="gate-caption">{current.label}</div> : null}
        </div>
        <div className="gate-sprockets" aria-hidden="true" />
      </div>
    </div>
  );
}
