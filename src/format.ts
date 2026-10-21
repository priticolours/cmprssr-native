// Human-friendly formatting helpers (decimal units, matching how storage is sold).

export function formatBytes(bytes: number): string {
  if (!bytes || bytes <= 0) return "0 MB";
  const gb = bytes / 1e9;
  if (gb >= 1) return `${gb.toFixed(gb >= 100 ? 0 : 1)} GB`;
  const mb = bytes / 1e6;
  if (mb >= 1) return `${mb.toFixed(mb >= 100 ? 0 : 1)} MB`;
  return `${Math.max(1, Math.round(bytes / 1e3))} KB`;
}

export function formatDuration(ms: number): string {
  const totalSeconds = Math.max(0, Math.round(ms / 1000));
  const h = Math.floor(totalSeconds / 3600);
  const m = Math.floor((totalSeconds % 3600) / 60);
  const s = totalSeconds % 60;
  if (h > 0) return `${h}h ${m}m`;
  if (m > 0) return `${m}m ${s}s`;
  return `${s}s`;
}

export function savedPercent(bytesIn: number, bytesOut: number): number {
  if (bytesIn <= 0) return 0;
  return Math.max(0, Math.round((1 - bytesOut / bytesIn) * 100));
}

export function formatCount(n: number): string {
  return n.toLocaleString();
}

export function formatHMS(ms: number): string {
  const totalSeconds = Math.max(0, Math.round(ms / 1000));
  const h = Math.floor(totalSeconds / 3600);
  const m = Math.floor((totalSeconds % 3600) / 60);
  const s = totalSeconds % 60;
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(h)}:${pad(m)}:${pad(s)}`;
}
