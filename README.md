# cmprssr-native

Raw CinemaDNG footage in, DNGs out — compressed enough to ship, still full raw
latitude, so DaVinci Resolve grades them like nothing happened. A native macOS
app: a Rust engine and a small web UI, no host framework wrapped around them.

**Built by** [Carl Perkins](https://github.com/priticolours).

## The honest thing up front

The compression isn't mine. cmprssr drives **Adobe DNG Converter** under the
hood — that's the thing that actually re-encodes the frames. I don't write the
codec; I write the scaffolding around it: finding the converter, feeding it
batches, reading back what it did, and refusing to report a number I can't
stand behind. Everything below is that scaffolding.

## What the engine does

Two Rust files — `src-tauri/src/dng.rs` for the orchestration, `tiff.rs` for a
minimal dependency-free TIFF reader. Four things matter.

**Parallelism, bounded on purpose.** Adobe's converter is already multithreaded,
so launching several at once just makes them fight for cores. The engine caps
concurrent batches at `max(2, cores / 2)` and sizes each batch at 64 frames to
amortize the per-invocation startup cost. More workers past that doesn't help —
it hurts. The converter is sequential per file, so the whole thing tops out
around 1.6× over serial; that's the converter's ceiling, not a bug I can tune
away.

**The ratio you're shown is the one that means something.** Whole-file size
lies. A DNG carries embedded previews and a thumbnail alongside the raw frame,
so a file-size ratio is mostly measuring JPEG you never touched. The engine
reads the actual raw frame's `TileByteCounts` and reports *that* — the honest
raw-to-raw number. It has to find the raw frame first; see the bug below.

**Already-compressed frames are skipped, not re-run.** If the target is lossless
or default and the frame is already Compression 7 (lossless JPEG, per DNG 1.4),
there's nothing to gain. cmprssr copies the original through so the output reel
stays complete and counts it as skipped — zero converter spawns for those.

**Output is verified before it's trusted.** Every output DNG is parsed back:
tile grid intact, crop present, byte counts consistent. If verification fails,
the original is passed through untouched and the frame is counted as failed
rather than shipped broken.

## The bug that wasn't obvious

Adobe nests the real frame in a SubIFD (tag 254 = 0, "full-resolution image")
and leaves a small thumbnail at IFD0. The engine was reading IFD0 — so the
ratio, the skip check, and the verify pass were all measuring a 256×144 preview
instead of the frame. The fix is to promote the frame marked full-resolution;
largest frame wins on ties, and plain TIFFs with no such marker keep IFD0. After
that, the reported ratio matches a manual sum of `TileByteCounts` to the byte.

## Proving it without the window

```
cmprssr --selftest <source> [dest] [mode] [recursive]
```

Runs one compress end-to-end against the real engine and prints the tallies as
JSON — no GUI. It exists because the engine is the part that has to be right,
and a headless run is easier to prove than a wizard you have to click through.
`mode` is `default` | `lossless` | `high`; `recursive` is `0` (top-level only,
default) or `1`. Exit 0 on success, 1 on engine failure, 2 on bad arguments.

```bash
./cmprssr --selftest ~/footage/reel1 /tmp/out default 0
```

## Build & run

```bash
npm install
npx tauri build        # release — this is what to use
```

You need the Rust toolchain and **Adobe DNG Converter** installed (the app finds
it on your PATH, or offers to download it).

`npx tauri dev` currently white-screens on boot — use the release build until
that's fixed.

## Where this is

Beta. The engine is the verified part — unit tests plus real Adobe output
through `--selftest`. The GUI is still being worked on: what's here builds and
runs (the five-step wizard plus an experimental home view), but the final look
and flow aren't settled yet.

## What's not here on purpose

- No native codec engine. Adobe's converter *is* the engine; replacing it is a
  different project.
- No downscale or proxy mode. Compression happens last and is never stacked with
  anything else.
- No settings window.
