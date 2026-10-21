import * as React from "react";
import { api } from "../lib/api";
import type { CompressionMode, JobResult, ProgressChunk, ScanResult } from "../types";
import { formatBytes, formatCount, formatHMS } from "../format";
import { FilmGate } from "./filmgate";
import type { GateFrame } from "./filmgate";

// ── Braun TG 60–inspired faceplate ─────────────────────────────────────────
// One hardware panel: LOAD / RUN / STOP pushbuttons, a huge tabular file
// counter, three quiet stat rows, one thin amber progress rule, and a small
// live MB/S meter window. No wizard, no reels.

type Phase = "idle" | "scanning" | "ready" | "running" | "canceling" | "done" | "error";

const MODES: CompressionMode[] = ["default", "lossless", "high"];
const MODE_LABEL: Record<CompressionMode, string> = {
  default: "DEFAULT",
  lossless: "LOSSLESS",
  high: "HIGH",
};

function makeId(): string {
  return `job-${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

function shortPath(p: string | null): string {
  if (!p) return "—";
  const parts = p.split("/");
  if (parts.length <= 3) return p;
  return "…/" + parts.slice(-2).join("/");
}

function baseName(p: string): string {
  const t = p.replace(/\/+$/, "");
  const i = t.lastIndexOf("/");
  return i >= 0 ? t.slice(i + 1) : t;
}

/** Default output: a sibling of the source named "<name>-cmprssd", so a
 *  dropped CINEMA folder lands next to it as CINEMA-cmprssd for easy A/B. */
function autoDest(sourcePath: string): string {
  const t = sourcePath.replace(/\/+$/, "");
  const i = t.lastIndexOf("/");
  const parent = i > 0 ? t.slice(0, i) : "/";
  const name = baseName(t);
  const out = name.endsWith("-cmprssd") ? name : `${name}-cmprssd`;
  return `${parent}/${out}`;
}

/** Real sticker PNGs (src/assets/stickers/*.png). Each one is printed three
 *  times with different color treatments — like different print runs of the
 *  same sticker — and scattered evenly across the faceplate. Deterministic
 *  so it looks identical every launch. UI panels sit above the bomb. */
const stickerUrls = Object.values(
  import.meta.glob("../assets/stickers/*.png", {
    eager: true,
    query: "?url",
    import: "default",
  }) as Record<string, string>,
);

const COPY_FILTERS = [
  "",
  "hue-rotate(38deg) saturate(1.3)",
  "hue-rotate(-32deg) brightness(1.1)",
];

function scatter(seedIn: number, extraFilter = ""): React.CSSProperties {
  // mulberry-ish deterministic PRNG seeded per sticker copy
  let seed = (0x9e3779b9 + seedIn * 0x85ebca6b) >>> 0;
  const rnd = () => {
    seed ^= seed << 13; seed >>>= 0;
    seed ^= seed >>> 17;
    seed ^= seed << 5; seed >>>= 0;
    return (seed % 1000) / 1000;
  };
  // vmin sizing keeps the bomb dense at any window size.
  const size =
    seedIn % 11 === 0 ? 20 + rnd() * 9
    : seedIn % 3 === 0 ? 12 + rnd() * 6
    : 8 + rnd() * 5;
  const shadow =
    "drop-shadow(0 4px 6px rgba(0,0,0,0.45)) drop-shadow(0 1px 2px rgba(0,0,0,0.3))";
  // Even spread: half the copies roam the full faceplate including the
  // window edges, half cluster nearer the middle so stickers tuck around
  // the instrument clusters without burying them.
  const edge = rnd() < 0.5;
  const left = edge ? -7 + rnd() * 104 : 5 + rnd() * 88;
  const top = edge ? -7 + rnd() * 104 : 8 + rnd() * 82;
  return {
    left: `${left.toFixed(1)}%`,
    top: `${top.toFixed(1)}%`,
    width: `${size.toFixed(1)}vmin`,
    transform: `rotate(${(rnd() * 32 - 16).toFixed(1)}deg)`,
    filter: extraFilter ? `${shadow} ${extraFilter}` : shadow,
    zIndex: seedIn,
  };
}

function StickerBomb() {
  if (stickerUrls.length === 0) return null;
  const copies: { url: string; seed: number; filter: string }[] = [];
  stickerUrls.forEach((url, i) => {
    COPY_FILTERS.forEach((filter, c) => {
      copies.push({ url, seed: i * COPY_FILTERS.length + c, filter });
    });
  });
  return (
    <div className="sticker-bomb" aria-hidden="true">
      {copies.map(({ url, seed, filter }, n) => (
        <img key={`${url}#${n}`} src={url} alt="" draggable={false}
          className="sticker-bomb-img" style={scatter(seed, filter)} />
      ))}
    </div>
  );
}

/** Small analog MB/S meter: cream face, needle, typewriter brand strip. */
function Meter({ mbps }: { mbps: number }) {
  const clamped = Math.max(0, Math.min(1, mbps / 60));
  const angle = -42 + 84 * clamped;
  const ticks = [-42, -21, 0, 21, 42];
  return (
    <div className="meter-wrap">
      <span className="faceplate-micro-label">MB/S</span>
      <svg viewBox="0 0 132 86" className="meter-bezel meter-svg">
        <rect x="2" y="2" width="128" height="82" rx="6" fill="#f4ecd8" />
        {ticks.map((t) => {
          const rad = ((t - 90) * Math.PI) / 180;
          const x1 = 66 + 44 * Math.cos(rad);
          const y1 = 66 + 44 * Math.sin(rad);
          const x2 = 66 + 52 * Math.cos(rad);
          const y2 = 66 + 52 * Math.sin(rad);
          return <line key={t} x1={x1} y1={y1} x2={x2} y2={y2} stroke="#4a4438" strokeWidth="1.5" />;
        })}
        <text x="66" y="34" textAnchor="middle" fontSize="9" fill="#4a4438" fontFamily="ui-monospace, Menlo, monospace">
          MB/S
        </text>
        <g transform={`rotate(${angle} 66 66)`}>
          <line x1="66" y1="66" x2="66" y2="20" stroke="#b3402a" strokeWidth="2" />
        </g>
        <circle cx="66" cy="66" r="4" fill="#2b2925" />
        <rect x="2" y="64" width="128" height="20" rx="0" fill="#2b2925" />
        <text x="66" y="78" textAnchor="middle" fontSize="11" fill="#f4ecd8"
          fontFamily="ui-monospace, Courier, monospace" letterSpacing="2">
          cmprssr
        </text>
      </svg>
    </div>
  );
}

function PushButton({ label, tone, active, attention, disabled, onClick }: {
  label: string; tone: "load" | "run" | "stop"; active?: boolean; attention?: boolean;
  disabled?: boolean; onClick: () => void;
}) {
  return (
    <div className="pushbutton-wrap">
      <button type="button" disabled={disabled} onClick={onClick}
        className={`pushbutton pushbutton-${tone}${active ? " pushbutton-active" : ""}${attention ? " pushbutton-attn" : ""}`}
        aria-label={label}>
        <span className={`pushbutton-dot ${active ? "pushbutton-dot-on" : ""}`} />
      </button>
      <span className="faceplate-micro-label">{label}</span>
    </div>
  );
}

const STATUS_TEXT: Record<Phase, string> = {
  idle: "IDLE",
  scanning: "SCANNING",
  ready: "READY",
  running: "COMPRESSING",
  canceling: "STOPPING",
  done: "DONE",
  error: "ERROR",
};

export function Faceplate() {
  const [phase, setPhase] = React.useState<Phase>("idle");
  const [sourcePath, setSourcePath] = React.useState<string | null>(null);
  const [destPath, setDestPath] = React.useState<string | null>(null);
  const [mode, setMode] = React.useState<CompressionMode>("default");
  const [modeTouched, setModeTouched] = React.useState(false);
  const [scan, setScan] = React.useState<ScanResult | null>(null);
  const [scanError, setScanError] = React.useState<string | null>(null);
  const [progress, setProgress] = React.useState<ProgressChunk | null>(null);
  const [result, setResult] = React.useState<JobResult | null>(null);
  const [runError, setRunError] = React.useState<string | null>(null);
  const [dropActive, setDropActive] = React.useState(false);
  const [destCustom, setDestCustom] = React.useState(false);
  const [gateFrames, setGateFrames] = React.useState<GateFrame[]>([]);
  const [workers, setWorkers] = React.useState(4);
  const jobIdRef = React.useRef<string | null>(null);
  const startedAtRef = React.useRef(0);
  const loadIdRef = React.useRef(0);
  const lastGatePathRef = React.useRef("");
  const seenGatePathsRef = React.useRef<Set<string>>(new Set());
  // Frame previews: the embedded JPEG when the file has one, otherwise a
  // tiny grayscale video-tap synthesized from the raw sensor data (tiff.rs).
  // Extractions run strictly one at a time through a FIFO queue, so a burst
  // of frames can't hammer the disk and every requested frame gets its turn.
  const thumbCache = React.useRef(new Map<string, string>());
  const thumbQueue = React.useRef<{ path: string; key: string }[]>([]);
  const thumbBusy = React.useRef(false);
  const thumbPending = React.useRef(new Set<string>()); // keys queued or in flight
  const pumpThumbs = React.useCallback(() => {
    if (thumbBusy.current) return;
    const next = thumbQueue.current.shift();
    if (!next) return;
    thumbBusy.current = true;
    api.thumbnail(next.path).then((t) => {
      if (t) {
        const c = thumbCache.current;
        if (c.size >= 256) {
          const oldest = c.keys().next().value;
          if (oldest !== undefined) c.delete(oldest);
        }
        c.set(next.path, t);
        setGateFrames((prev) => prev.map((f) => (f.key === next.key ? { ...f, thumb: t } : f)));
      }
    }).catch(() => { /* no preview — the filename label stays */ }).finally(() => {
      thumbBusy.current = false;
      thumbPending.current.delete(next.key);
      pumpThumbs();
    });
  }, []);
  const clearThumbs = React.useCallback(() => {
    thumbQueue.current = [];
    thumbPending.current.clear();
  }, []);
  const fetchThumb = React.useCallback((path: string, key: string) => {
    const hit = thumbCache.current.get(path);
    if (hit) {
      setGateFrames((prev) => prev.map((f) => (f.key === key ? { ...f, thumb: hit } : f)));
      return;
    }
    if (thumbPending.current.has(key)) return; // already queued or in flight
    thumbPending.current.add(key);
    thumbQueue.current.push({ path, key });
    pumpThumbs();
  }, [pumpThumbs]);
  const [, forceTick] = React.useReducer((x: number) => x + 1, 0);

  // Keep the clock + meter alive while running.
  React.useEffect(() => {
    if (phase !== "running" && phase !== "canceling") return;
    const t = window.setInterval(forceTick, 500);
    return () => window.clearInterval(t);
  }, [phase]);

  const loadSource = async (picked: string) => {
    if (phase === "running" || phase === "canceling" || phase === "scanning") return;
    const loadId = ++loadIdRef.current;
    setSourcePath(picked);
    try { localStorage.setItem("cmprssr.source", picked); } catch { /* ignore */ }
    setDestPath(null);
    setDestCustom(false);
    setScan(null);
    setScanError(null);
    setResult(null);
    setRunError(null);
    setProgress(null);
    setGateFrames([]);
    clearThumbs();
    lastGatePathRef.current = "";
    seenGatePathsRef.current = new Set();
    setPhase("scanning");
    try {
      const r = await api.scan(picked, true);
      if (loadId !== loadIdRef.current) return; // superseded by a newer load
      setScan(r);
      setPhase(r.totalFiles > 0 ? "ready" : "idle");
      if (r.totalFiles === 0) setScanError("No DNG frames found in that folder.");
      // Idle gate: thread the first frames of the reel through.
      const samples = (r.samplePaths ?? []).slice(0, 8);
      setGateFrames(samples.map((p, i) => ({
        key: `idle-${loadId}-${i}`,
        thumb: null,
        label: baseName(p),
      })));
      samples.forEach((p, i) => {
        if (loadId === loadIdRef.current) fetchThumb(p, `idle-${loadId}-${i}`);
      });
    } catch (err) {
      if (loadId !== loadIdRef.current) return;
      setScanError(err instanceof Error ? err.message : "Couldn't read that folder.");
      setPhase("idle");
    }
  };

  const chooseSource = async () => {
    const picked = await api.chooseFolder("Choose your footage folder");
    if (picked) await loadSource(picked);
  };

  const chooseDest = async () => {
    if (phase === "running" || phase === "canceling") return;
    const picked = await api.chooseFolder("Choose where to save compressed footage");
    if (picked) { setDestPath(picked); setDestCustom(true); }
  };

  const cycleMode = () => {
    if (phase === "running" || phase === "canceling") return;
    setModeTouched(true);
    setMode((m) => MODES[(MODES.indexOf(m) + 1) % MODES.length]);
  };

  // Parallel converter workers: fewer runs cooler, more runs faster.
  const stepWorkers = (d: number) => {
    if (phase === "running" || phase === "canceling") return;
    setWorkers((w) => Math.min(8, Math.max(1, w + d)));
  };

  const runJob = () => {
    if (!sourcePath || phase === "running" || phase === "canceling" || phase === "scanning") return;
    const id = makeId();
    jobIdRef.current = id;
    startedAtRef.current = Date.now();
    setProgress(null);
    setResult(null);
    setRunError(null);
    setPhase("running");
    lastGatePathRef.current = "";
    seenGatePathsRef.current = new Set();
    const srcName = baseName(sourcePath);
    const outName = srcName.endsWith("-cmprssd") ? srcName : `${srcName}-cmprssd`;
    // Default output is a sibling named <source>-cmprssd; a custom destination
    // becomes the parent of that folder, never the flat dump of files.
    const dest = destCustom && destPath
      ? `${destPath.replace(/\/+$/, "")}/${outName}`
      : autoDest(sourcePath);
    api.runCompression(
      { sourceDir: sourcePath, destDir: dest, mode, recursive: true, workers },
      (chunk) => {
        setProgress(chunk);
        // Pull each finished frame through the gate once.
        const p = chunk.currentPath;
        if (p && p !== lastGatePathRef.current && !seenGatePathsRef.current.has(p)) {
          seenGatePathsRef.current.add(p);
          lastGatePathRef.current = p;
          const key = `run-${id}-${seenGatePathsRef.current.size}`;
          const label = baseName(p);
          setGateFrames((prev) => [...prev.slice(-11), { key, thumb: null, label }]);
          fetchThumb(p, key);
        }
      },
      id,
    ).then((r) => {
      setResult(r);
      setPhase("done");
    }).catch((err: unknown) => {
      setRunError(err instanceof Error ? err.message : "Something went wrong while compressing.");
      setPhase("error");
    });
  };

  const stopJob = () => {
    if (phase !== "running" || !jobIdRef.current) return;
    setPhase("canceling");
    api.cancel(jobIdRef.current);
  };

  const reset = () => {
    jobIdRef.current = null;
    loadIdRef.current += 1;
    setSourcePath(null);
    setDestPath(null);
    setDestCustom(false);
    setMode("default");
    setModeTouched(false);
    setScan(null);
    setScanError(null);
    setProgress(null);
    setResult(null);
    setRunError(null);
    setGateFrames([]);
    clearThumbs();
    lastGatePathRef.current = "";
    seenGatePathsRef.current = new Set();
    try { localStorage.removeItem("cmprssr.source"); } catch { /* ignore */ }
    setPhase("idle");
  };

  // Latest actions for keyboard shortcuts (avoids stale closures).
  const actionsRef = React.useRef({ chooseSource, runJob, stopJob, cycleMode, loadSource });
  actionsRef.current = { chooseSource, runJob, stopJob, cycleMode, loadSource };

  // Remember the quality mode between sessions.
  React.useEffect(() => {
    try { localStorage.setItem("cmprssr.mode", mode); } catch { /* ignore */ }
  }, [mode]);

  // Restore last session, folder drag-and-drop, keyboard transport.
  React.useEffect(() => {
    try {
      const s = localStorage.getItem("cmprssr.source");
      if (s) {
        // Mode only comes back with a source; otherwise it stays DEFAULT.
        const m = localStorage.getItem("cmprssr.mode");
        if (m === "default" || m === "lossless" || m === "high") {
          setMode(m);
          setModeTouched(true);
        }
        void actionsRef.current.loadSource(s);
      }
    } catch { /* ignore */ }

    let unlisten: (() => void) | undefined;
    (async () => {
      try {
        const { getCurrentWebviewWindow } = await import("@tauri-apps/api/webviewWindow");
        unlisten = await getCurrentWebviewWindow().onDragDropEvent((event) => {
          if (event.payload.type === "enter") setDropActive(true);
          else if (event.payload.type === "leave") setDropActive(false);
          else if (event.payload.type === "drop") {
            setDropActive(false);
            const p = event.payload.paths[0];
            if (p) void actionsRef.current.loadSource(p);
          }
        });
      } catch { /* not running inside a Tauri webview */ }
    })();

    const onKey = (e: KeyboardEvent) => {
      if (e.metaKey || e.ctrlKey || e.altKey || e.repeat) return;
      const k = e.key.toLowerCase();
      if (k === "l") void actionsRef.current.chooseSource();
      else if (k === "r") actionsRef.current.runJob();
      else if (k === "s" || k === "escape") actionsRef.current.stopJob();
      else if (k === "m") actionsRef.current.cycleMode();
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
      unlisten?.();
    };
  }, []);

  // ── Derived readouts ────────────────────────────────────────────────
  const total = progress?.totalFiles || scan?.totalFiles || 0;
  const done = progress?.filesDone ?? (phase === "done" ? result?.filesProcessed ?? 0 : 0);
  const bytesIn = progress?.bytesIn ?? result?.bytesIn ?? 0;
  const bytesOut = progress?.bytesOut ?? result?.bytesOut ?? 0;
  const failed = progress?.failed ?? result?.filesFailed ?? 0;
  const skipped = progress?.filesSkipped ?? result?.filesSkipped ?? 0;

  const elapsedMs =
    phase === "running" || phase === "canceling"
      ? Date.now() - startedAtRef.current
      : (result?.elapsedMs ?? 0);
  const mbps = elapsedMs > 500 ? bytesOut / 1e6 / (elapsedMs / 1000) : 0;

  const liveRatio = bytesIn > 0 && bytesOut > 0 ? bytesIn / bytesOut : 0;
  const ratioText =
    phase === "done" && result
      ? `${result.trueRatio.toFixed(2)} : 1`
      : liveRatio > 0 ? `${liveRatio.toFixed(2)} : 1` : "—";

  const sizeText =
    bytesIn > 0 || bytesOut > 0
      ? `${formatBytes(bytesIn)} → ${formatBytes(bytesOut)}`
      : scan ? `${formatBytes(scan.totalBytes)} → —` : "—";

  // ETA from file throughput (robust while byte estimates are still settling).
  const filesPerSec = elapsedMs > 2000 && done > 0 ? done / (elapsedMs / 1000) : 0;
  const remaining = Math.max(0, total - done);
  const etaText =
    phase === "running" && filesPerSec > 0 && remaining > 0
      ? formatHMS((remaining / filesPerSec) * 1000)
      : phase === "done" ? formatHMS(result?.elapsedMs ?? 0) : "—";

  const effectiveDest = sourcePath
    ? (destCustom && destPath
        ? `${destPath.replace(/\/+$/, "")}/${(() => { const n = baseName(sourcePath); return n.endsWith("-cmprssd") ? n : `${n}-cmprssd`; })()}`
        : autoDest(sourcePath))
    : null;

  const percent = total > 0 ? Math.min(1, done / total) : 0;
  const statusDot = phase === "running" || phase === "canceling";
  const doneCount = String(done).padStart(4, "0");
  const canRun = (phase === "ready" || phase === "done" || phase === "error") && total > 0;
  return (
    <div className="faceplate-stage">
      <div className="faceplate">
        <div className="screws" aria-hidden="true">
          <span className="screw screw-tl" />
          <span className="screw screw-tr" />
          <span className="screw screw-bl" />
          <span className="screw screw-br" />
        </div>
        <StickerBomb />
        <div className="faceplate-stamp">@rt.WORK</div>

        <div className="faceplate-mid">
          <div className="cluster cluster-info">
            <div className="faceplate-micro-label">FILE</div>
            <div className="faceplate-counter">
              {doneCount} <span className="faceplate-counter-sep">/</span> {formatCount(total)}
            </div>
            <div className="faceplate-rule">
              <div className="faceplate-rule-fill" style={{ width: `${percent * 100}%` }} />
            </div>
            <div className="faceplate-stat-row">
              <span className="faceplate-stat-label">TRUE RATIO</span>
              <span className="faceplate-stat-value">{ratioText}</span>
            </div>
            <div className="faceplate-stat-row">
              <span className="faceplate-stat-label">SIZE</span>
              <span className="faceplate-stat-value">{sizeText}</span>
            </div>
            <div className="faceplate-stat-row">
              <span className="faceplate-stat-label">ETA</span>
              <span className="faceplate-stat-value">{etaText}</span>
            </div>
          </div>

          <div className="cluster">
            <Meter mbps={mbps} />
            <div className="faceplate-status">
              <span className={`faceplate-led ${statusDot ? "faceplate-led-on" : ""}`} />
              <span className="faceplate-status-text">{STATUS_TEXT[phase]}</span>
            </div>
          </div>

          <div className="cluster">
            <div className="faceplate-micro-label">TRANSPORT</div>
            <div className="faceplate-transport">
              <PushButton label="LOAD" tone="load" onClick={chooseSource}
                attention={!sourcePath && phase !== "scanning"} />
              <PushButton label="RUN" tone="run" active={phase === "running" || phase === "canceling"}
                disabled={!canRun} onClick={runJob} />
              <PushButton label="STOP" tone="stop" active={phase === "canceling"}
                disabled={phase !== "running"} onClick={stopJob} />
            </div>
          </div>

          <div className="cluster cluster-gate">
            <FilmGate
              frames={gateFrames}
              running={phase === "running" || phase === "canceling"}
              emptyLabel={sourcePath ? "THREADING…" : "NO FILM IN GATE"}
            />
          </div>
        </div>
        {failed > 0 || skipped > 0 ? (
          <div className="faceplate-note">
            {skipped > 0 ? `${formatCount(skipped)} already compressed — copied through` : null}
            {skipped > 0 && failed > 0 ? " · " : null}
            {failed > 0 ? `${formatCount(failed)} failed — originals kept` : null}
          </div>
        ) : null}

        <div className="faceplate-setup">
          <button type="button" className="faceplate-quiet" onClick={chooseSource} title={sourcePath ?? ""}>
            IN&nbsp;&nbsp;{shortPath(sourcePath)}
          </button>
          <button type="button"
            className={`faceplate-quiet${sourcePath && !destCustom ? " faceplate-quiet-attn" : ""}`}
            onClick={chooseDest} title={effectiveDest ?? ""}>
            OUT&nbsp;&nbsp;{effectiveDest ? `${shortPath(effectiveDest)}${destCustom ? "" : " · AUTO"}` : "—"}
          </button>
          <button type="button"
            className={`faceplate-quiet${!modeTouched ? " faceplate-quiet-attn" : ""}`}
            onClick={cycleMode}>
            MODE&nbsp;&nbsp;{MODE_LABEL[mode]}
          </button>
          <span className="faceplate-quiet faceplate-quiet-static" title="Parallel converter workers — fewer runs cooler">
            WORKERS&nbsp;&nbsp;{/*
            */}<button type="button" className="faceplate-stepper" aria-label="fewer workers"
              disabled={workers <= 1 || phase === "running" || phase === "canceling"}
              onClick={() => stepWorkers(-1)}>−</button>
            <span className="faceplate-stepper-val">{workers}</span>
            <button type="button" className="faceplate-stepper" aria-label="more workers"
              disabled={workers >= 8 || phase === "running" || phase === "canceling"}
              onClick={() => stepWorkers(1)}>+</button>
          </span>
          {phase === "done" && result ? (
            <button type="button" className="faceplate-quiet"
              onClick={() => void api.reveal(result.outputDir)}>
              REVEAL&nbsp;&nbsp;OUTPUT
            </button>
          ) : null}
          {phase === "done" || phase === "error" ? (
            <button type="button" className="faceplate-quiet" onClick={reset}>
              RESET
            </button>
          ) : null}
          <span className="faceplate-quiet faceplate-quiet-static" title="Keyboard shortcuts">
            KEYS&nbsp;&nbsp;L&nbsp;·&nbsp;R&nbsp;·&nbsp;S&nbsp;·&nbsp;M
          </span>
        </div>

        {scanError || runError ? (
          <div className="faceplate-error">{scanError ?? runError}</div>
        ) : null}
        <div className="faceplate-texture" aria-hidden="true" />
        {dropActive ? (
          <div className="faceplate-drop-overlay">
            <span>DROP FOOTAGE FOLDER</span>
          </div>
        ) : null}
      </div>
    </div>
  );
}
