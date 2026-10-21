# About this port

cmprssr-native is a faithful port of the original
[cmprssr](https://github.com/priticolours/cmprssr) — the Glaze-hosted Electron
version — rebuilt as a standalone Tauri 2 desktop app. Same app, the Glaze host
layer cut out from underneath it. No new features in the port; that was the
point.

## What moved over

- The full five-step wizard (source → quality → destination → progress → done):
  all nine components, the home view, router, theme tokens, and copy — one-to-one
  with the original, with the `@glaze/*` imports swapped for local equivalents.
- The DNG backend in Rust (`src-tauri/src/dng.rs` and `tiff.rs`): engine
  discovery (PATH plus the two canonical Adobe install locations), Homebrew
  install with one retry, recursive scan, DNG/companion splitting, batched
  converter runs, output verification with original-passthrough on failure,
  companion copying, progress events, and cancellation.
- The Tauri commands: `check_engine`, `install_engine`, `pick_folder`,
  `run_compress`, `cancel_compress`, `open_path`, `open_url`, plus the
  `cmprssr://progress/<job_id>` event channel.

Since the port, the engine is the part that got worked on — bounded parallelism,
the honest tile-payload ratio, skip-if-compressed, the verify pass, and the
SubIFD fix. All of it is in the README above.

## What was left out on purpose

- **The settings window.** In the original it was only a generic host theme
  selector plus a second-window shell — nothing app-specific. Theme now follows
  the OS color scheme (`src/lib/hooks.ts`).
- **Dev-only Glaze plumbing** (`useGlazeDevStatus`, the backend-disconnect
  overlay) is stubbed and its indicators hidden.

## Where it stands

The engine is proven: `cargo test` passes (34 unit tests, including the SubIFD
regression), `tsc --noEmit` is clean, `vite build` succeeds, and `grep -ri glaze
src` comes back empty — nothing Glaze survived the port.

What hasn't been clicked through yet is the GUI end to end on real hardware:
engine auto-detect, the install path, folder picking, a live DNG batch, cancel
mid-run, the done screen. The engine itself is verified through `--selftest`
(README); clicking the wizard through is the list of things still to confirm.
