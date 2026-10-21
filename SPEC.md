# cmprssr — phase 1 build spec

approved 2026-09-23 by priti. goal: slimraw-class speed and quality, UI that blows it out of the water.

## 1. parallel batch execution (speed)

why: the current loop awaits one 16-file converter run at a time. this is the single biggest speed lever.

- bounded parallelism via `tokio::sync::Semaphore`. default permits = `max(2, available_parallelism / 2)` — adobe's converter is itself multithreaded, oversubscription hurts. expose as `parallelism` in CompressParams; UI control 1–8, default auto.
- each 16-file batch becomes a task; aggregate progress with atomics (`files_done`, `bytes_in`, `bytes_out`, `failed`, `skipped`) plus one progress-emit task on a 250ms interval. replaces the serial `send_chunk` callback.
- cancellation: existing CancellationToken checked per task; on cancel, kill children (existing logic), then drain.
- keep `BATCH_SIZE = 16` (amortizes process spawn cost). benchmark batch/worker counts later.
- verify: multiple converter processes writing distinct filenames into one out folder — no collisions expected (names unique per source folder), but test it.

## 2. honest ratios from tile payload bytes (quality reporting)

why: today's ratio divides whole file sizes; embedded previews and IFD data pollute it. the fpSup research warns against exactly this.

- new module `src-tauri/src/tiff.rs`: minimal TIFF/IFD reader, no new deps. parse header (II/MM), walk IFD0 + SubIFDs (tag 330). read tags:
  - 259 Compression, 258 BitsPerSample
  - 322 TileWidth, 323 TileLength, 324 TileOffsets, 325 TileByteCounts
  - 273 StripOffsets, 279 StripByteCounts (fallback for stripped DNGs)
  - 511 DefaultCropOrigin, 512 DefaultCropSize (verify pass)
- `payload_bytes(path) -> u64` = sum of TileByteCounts (or StripByteCounts).
- scan phase sums payload bytes per folder → pre-run estimate: `est_out = payload / 2.0`, show the honest range 1.65–2.6. display "≈ 38.2 GB → ≈ 19.1 GB".
- `JobResult.ratio` becomes payload-based (headline number); keep whole-file ratio as a secondary line.
- unit tests: synthesize a minimal TIFF in-test, parse it back.

## 3. skip-if-compressed (intelligence)

- during scan, read Compression (259):
  - target lossless/default and Compression == 7 (lossless JPEG per DNG 1.4) → skip: copy the original to dest so the output reel stays complete; count `files_skipped`.
  - target lossy → always process (don't trust our read of lossy markers yet).
- `JobResult` gains `files_skipped`; the done screen lists them ("already compressed — passed through untouched").

## 4. verify pass (trust)

- post-run, per output DNG: parse IFD; assert TileLength % 16 == 0 (tiled), tile count ≤ 326, DefaultCropSize nonzero, required tags present. on failure: existing original-passthrough + count as failed-verify.
- phase 2: xxhash checksums for DIT-grade offload.

> **2026-10-20: §5 direction superseded.** The Braun TG-60 machine look is
> dropped; the final UI is Hermes-styled (dark, CSS-var themed, web UI). The
> engine work in §1–§4 (parallelism, honest ratio, skip, verify) stands and is
> the kept core.

## 5. UI — braun TG 60

2026-09-23: priti scratched the reel-to-reel/scanstation machine look, picked braun minimal, then asked for Braun TG 60 hardware details. no reels, no tape — just the controls.

the interface is a TG 60 faceplate: light warm-grey, dark charcoal type, strict grid, enormous whitespace.

- hero: enormous tabular-mono file counter — 0123 / 1456 — with a small letterspaced FILE label above.
- below: three quiet rows — TRUE RATIO 1.94 : 1, 38.2 GB → 19.7 GB, ETA 00:12:44. small-caps grotesk labels, tabular numerals.
- one thin progress rule: amber fill for the completed share, faint grey hairline for the rest.
- transport: a row of chunky square pushbuttons — LOAD / RUN / STOP — like the TG 60's. the active button carries a small amber dot (the record-dot gesture). tiny labels beneath, nothing else.
- one small rectangular meter window (the TG 60's VU meter) showing live MB/S throughput.
- wizard steps become plain labeled sections (source, destination, quality). quality = three plainly labeled options, no marketing copy.
- done screen: a plain lab report — final true ratio, GB saved, MB/s, skipped/failed counts with originals-preserved note. buttons: REVEAL IN FINDER, NEW REEL.
- signal color: single amber accent for active states; charcoal/grey for the rest; red only for failures.
- motion: odometer-rolling numerals, the rule fills linearly, the meter needle moves. nothing bounces, nothing sparkles.
- type: grotesk labels, tabular mono for all numerals.

## non-goals (phase 1)

- native codec engine (phase 2+)
- downscale/proxy mode — separate feature, never stacked with compression ("compression happens last")
- settings window

## acceptance

- 4 parallel batches on an 8-core mac: wall time ≤ 40% of serial on the same clips, identical output hashes vs serial.
- reported ratio on sigma fp clips matches manual TileByteCounts math.
- already-compressed folder: 100% skipped, output complete, zero converter spawns.
- a deliberately corrupted output is caught by verify and replaced with the original.
