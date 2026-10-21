//! DNG compression engine.
//!
//! Faithful port of `main/services/dng-converter.ts` from the Glaze app.
//! Wraps Adobe DNG Converter's command-line binary to (re)compress CinemaDNG
//! frame sequences. Adobe DNG Converter is a GUI app that ships a CLI binary
//! inside its bundle — it is NOT on $PATH, so we locate it by bundle path.
//!
//! Non-DNG files (audio, metadata, and anything else) are copied through
//! unchanged so the output clip folders stay complete.

use regex::Regex;
use serde::Serialize;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::process::Command;
use tokio::sync::{mpsc, Semaphore};
use tokio_util::sync::CancellationToken;

use crate::tiff;

// ── Types (mirrored in src/types.ts) ─────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineStatus {
    pub installed: bool,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    pub total_files: u64,
    pub total_bytes: u64,
    pub folder_count: u64,
    /// Full paths of up to 8 DNG files, for the film-gate idle preview.
    pub sample_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressChunk {
    pub files_done: u64,
    pub total_files: u64,
    pub current_file: String,
    /// Full path of the file being worked on (for the film-gate thumbnail).
    pub current_path: String,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub failed: u64,
    pub files_skipped: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobResult {
    pub files_processed: u64,
    pub files_failed: u64,
    pub files_copied: u64,
    pub files_skipped: u64,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub ratio: f64, // whole-file original / compressed, e.g. 2.1 means 2.1:1
    pub image_bytes_in: u64, // sum of TileByteCounts (fallback StripByteCounts)
    pub image_bytes_out: u64,
    pub true_ratio: f64, // image-data original / compressed — the honest number
    pub elapsed_ms: u64,
    pub output_dir: String,
    pub canceled: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallResult {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ── Engine location ──────────────────────────────────────────────────

pub const ENGINE_CANDIDATES: &[&str] = &[
    "/Applications/Adobe DNG Converter.app/Contents/MacOS/Adobe DNG Converter",
    "/Applications/Adobe/Adobe DNG Converter.app/Contents/MacOS/Adobe DNG Converter",
];

pub const BREW_CANDIDATES: &[&str] = &["/opt/homebrew/bin/brew", "/usr/local/bin/brew"];

// Compression flags per user-facing quality tier.
//   -c      lossless (JPEG lossless) DNG
//   -lossy  Adobe lossy compressed DNG (still raw-gradeable in Resolve)
//   -p0/-p1 embedded JPEG preview size (none / medium) — smaller vs. compatible
//   -fl     embed fast-load data for smoother scrubbing in NLEs
pub fn mode_args(mode: &str) -> Option<&'static [&'static str]> {
    match mode {
        "default" => Some(&["-c", "-p1", "-fl"]),
        "lossless" => Some(&["-c", "-p0"]),
        "high" => Some(&["-lossy", "-p1", "-fl"]),
        _ => None,
    }
}

pub const BATCH_SIZE: usize = 64;

pub fn find_engine() -> Option<String> {
    ENGINE_CANDIDATES
        .iter()
        .find(|candidate| Path::new(candidate).exists())
        .map(|s| s.to_string())
}

pub fn check_engine() -> EngineStatus {
    let engine_path = find_engine();
    EngineStatus {
        installed: engine_path.is_some(),
        path: engine_path,
    }
}

fn is_transient_download_error(message: &str) -> bool {
    Regex::new(r"(?i)partial file|curl: \(18\)|curl: \(28\)|connection reset|timed out")
        .expect("valid regex")
        .is_match(message)
}

/// Adobe's cask installs via a signed .pkg, which macOS only lets root install
/// through an interactive admin-password prompt. A background process has no
/// terminal to prompt on, so `sudo installer -pkg` fails here on every Mac —
/// this is not something a retry can fix.
fn is_privileged_installer_error(message: &str) -> bool {
    Regex::new(r"(?i)sudo|installer -pkg|Failure while executing.*installer")
        .expect("valid regex")
        .is_match(message)
}

/// Homebrew's error surfaces the full install log (progress bars, unrelated
/// dependency output, etc). Reduce it to one actionable line for the UI.
fn summarize_install_error(raw: &str) -> String {
    if is_privileged_installer_error(raw) {
        return "This installer needs your Mac password to finish, which we can't ask for in the background. Use the manual download below — it'll prompt you the normal way.".to_string();
    }
    if is_transient_download_error(raw) {
        return "Adobe's download was interrupted partway through, even after a retry. This usually clears up on its own — try again in a moment, or use the manual download below.".to_string();
    }
    let re = Regex::new(r"Error:[^\n]*").expect("valid regex");
    let matches: Vec<&str> = re.find_iter(raw).map(|m| m.as_str()).collect();
    if let Some(last) = matches.last() {
        let line = last.trim();
        let chars: Vec<char> = line.chars().collect();
        if chars.len() > 220 {
            return format!("{}…", chars[..220].iter().collect::<String>());
        }
        return line.to_string();
    }
    "Homebrew couldn't install it. Try again, or use the manual download below.".to_string()
}

async fn run_brew_install(brew: &str) -> Result<(), String> {
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(10 * 60),
        Command::new(brew)
            .args(["install", "--cask", "adobe-dng-converter"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output(),
    )
    .await
    .map_err(|_| "Homebrew install timed out after 10 minutes.".to_string())?
    .map_err(|e| format!("Failed to launch Homebrew: {e}"))?;

    if output.status.success() {
        return Ok(());
    }
    let mut message = String::from_utf8_lossy(&output.stderr).to_string();
    if message.trim().is_empty() {
        message = String::from_utf8_lossy(&output.stdout).to_string();
    }
    Err(format!(
        "Command failed: {brew} install --cask adobe-dng-converter\n{}",
        message.trim()
    ))
}

/// Install Adobe DNG Converter via Homebrew cask. Returns success/error rather
/// than throwing so the UI can show a friendly message and fall back to a
/// manual download. Retries once automatically since Adobe's CDN occasionally
/// drops the download mid-transfer (curl exit 18).
pub async fn install_engine() -> InstallResult {
    let brew = BREW_CANDIDATES
        .iter()
        .find(|p| Path::new(p).exists())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "brew".to_string());
    let mut last_message = String::new();

    for attempt in 1..=2 {
        match run_brew_install(&brew).await {
            Ok(()) => {
                let status = check_engine();
                if status.installed {
                    return InstallResult {
                        success: true,
                        error: None,
                    };
                }
                last_message =
                    "Install finished but the app wasn't found. Try installing manually.".to_string();
                break;
            }
            Err(message) => {
                eprintln!("[dng] Homebrew install failed (attempt {attempt}): {message}");
                last_message = message;
                if attempt == 1 && is_transient_download_error(&last_message) {
                    continue;
                }
                break;
            }
        }
    }

    InstallResult {
        success: false,
        error: Some(summarize_install_error(&last_message)),
    }
}

// ── Scanning ─────────────────────────────────────────────────────────

pub struct FolderEntry {
    pub rel_dir: PathBuf, // relative to sourceDir (empty for root)
    pub dng_files: Vec<PathBuf>,
    pub other_files: Vec<PathBuf>,
}

fn is_dng(name: &str) -> bool {
    name.to_lowercase().ends_with(".dng")
}

fn walk<'a>(
    dir: PathBuf,
    rel_dir: PathBuf,
    recursive: bool,
    entries: &'a mut Vec<FolderEntry>,
) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
    Box::pin(async move {
        let mut rd = tokio::fs::read_dir(&dir)
            .await
            .map_err(|e| e.to_string())?;
        let mut dng_files: Vec<PathBuf> = Vec::new();
        let mut other_files: Vec<PathBuf> = Vec::new();
        let mut sub_dirs: Vec<(PathBuf, PathBuf)> = Vec::new();

        while let Some(entry) = rd.next_entry().await.map_err(|e| e.to_string())? {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with('.') {
                continue; // skip dotfiles (.DS_Store etc.)
            }
            let abs = entry.path();
            let file_type = entry.file_type().await.map_err(|e| e.to_string())?;
            if file_type.is_dir() {
                sub_dirs.push((abs, rel_dir.join(name.as_os_str())));
            } else if file_type.is_file() {
                if is_dng(&name_str) {
                    dng_files.push(abs);
                } else {
                    other_files.push(abs);
                }
            }
        }

        if !dng_files.is_empty() || !other_files.is_empty() {
            entries.push(FolderEntry {
                rel_dir,
                dng_files,
                other_files,
            });
        }

        if recursive {
            for (abs, rel) in sub_dirs {
                walk(abs, rel, recursive, entries).await?;
            }
        }
        Ok(())
    })
}

async fn scan_folders(source_dir: &Path, recursive: bool) -> Result<Vec<FolderEntry>, String> {
    let mut entries = Vec::new();
    walk(
        source_dir.to_path_buf(),
        PathBuf::new(),
        recursive,
        &mut entries,
    )
    .await?;
    Ok(entries)
}

pub async fn scan(source_dir: &str, recursive: bool) -> Result<ScanResult, String> {
    let folders = scan_folders(Path::new(source_dir), recursive).await?;
    let mut total_files: u64 = 0;
    let mut total_bytes: u64 = 0;
    let mut folder_count: u64 = 0;
    let mut sample_paths: Vec<String> = Vec::new();

    for folder in &folders {
        if folder.dng_files.is_empty() {
            continue;
        }
        folder_count += 1;
        total_files += folder.dng_files.len() as u64;
        for file in &folder.dng_files {
            // ignore unreadable files in the estimate
            if let Ok(meta) = tokio::fs::metadata(file).await {
                total_bytes += meta.len();
            }
            if sample_paths.len() < 8 {
                sample_paths.push(file.to_string_lossy().to_string());
            }
        }
    }

    Ok(ScanResult {
        total_files,
        total_bytes,
        folder_count,
        sample_paths,
    })
}

// ── Compression ──────────────────────────────────────────────────────

fn chunk<T: Clone>(arr: &[T], size: usize) -> Vec<Vec<T>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < arr.len() {
        out.push(arr[i..(i + size).min(arr.len())].to_vec());
        i += size;
    }
    out
}

async fn file_size(p: &Path) -> u64 {
    tokio::fs::metadata(p).await.map(|m| m.len()).unwrap_or(0)
}

fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default()
}

fn run_converter<'a>(
    binary: &'a str,
    args: &'a [String],
    token: &'a CancellationToken,
) -> impl Future<Output = ()> + 'a {
    async move {
        if token.is_cancelled() {
            return;
        }
        let mut child = match Command::new(binary)
            .args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(child) => child,
            Err(err) => {
                eprintln!("[dng] Converter batch failed to spawn: {err}");
                return;
            }
        };
        tokio::select! {
            _ = child.wait() => {},
            _ = token.cancelled() => {
                let _ = child.kill().await;
                let _ = child.wait().await;
            }
        }
    }
}

fn progress_chunk(
    files_done: u64,
    total_files: u64,
    current_file: String,
    current_path: String,
    bytes_in: u64,
    bytes_out: u64,
    failed: u64,
    files_skipped: u64,
) -> ProgressChunk {
    ProgressChunk {
        files_done,
        total_files,
        current_file,
        current_path,
        bytes_in,
        bytes_out,
        failed,
        files_skipped,
    }
}

/// Upper bound on parallel converter batches (matches the UI's 1–8 range).
pub const MAX_WORKERS: usize = 8;

/// Resolve the worker count for a job.
///
/// `requested == 0` means Auto: `max(2, available_parallelism / 2)`.
/// Explicit values are clamped to 1–8.
pub fn resolve_workers(requested: usize) -> usize {
    if requested == 0 {
        let cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        (cores / 2).max(2).min(MAX_WORKERS)
    } else {
        requested.clamp(1, MAX_WORKERS)
    }
}

/// One converter invocation: a batch of inputs landing in one output folder.
struct BatchPlan {
    out_folder: PathBuf,
    inputs: Vec<PathBuf>,
}

/// What a batch worker reports back to the progress aggregator.
struct BatchTally {
    done: u64,
    failed: u64,
    skipped: u64,
    bytes_in: u64,
    bytes_out: u64,
    image_bytes_in: u64,
    image_bytes_out: u64,
    current_file: String,
    current_path: String,
}

/// Inspect one input on a blocking thread. Returns the parsed report, if the
/// file is a readable TIFF/DNG at all.
async fn inspect_input(path: PathBuf) -> Option<tiff::FileReport> {
    tokio::task::spawn_blocking(move || tiff::inspect(&path).ok())
        .await
        .ok()
        .flatten()
}

/// Run one batch: skip already-compressed inputs (default/lossless modes),
/// convert the rest, verify every output, and copy the original through
/// whenever anything goes wrong so no frame is ever lost.
async fn run_batch(
    engine: String,
    args_prefix: Vec<String>,
    plan: BatchPlan,
    allow_skip: bool,
    token: CancellationToken,
) -> BatchTally {
    let mut tally = BatchTally {
        done: 0,
        failed: 0,
        skipped: 0,
        bytes_in: 0,
        bytes_out: 0,
        image_bytes_in: 0,
        image_bytes_out: 0,
        current_file: String::new(),
        current_path: String::new(),
    };

    // Classify inputs first: already-compressed files are copied through.
    let mut to_convert: Vec<(PathBuf, u64, Option<tiff::FileReport>)> = Vec::new();
    for input in &plan.inputs {
        if token.is_cancelled() {
            return tally;
        }
        let in_size = file_size(input).await;
        tally.bytes_in += in_size;
        let report = inspect_input(input.clone()).await;
        let compressed = allow_skip
            && report
                .as_ref()
                .map(|r| r.main.already_compressed())
                .unwrap_or(false);
        if compressed {
            let out_path = plan.out_folder.join(file_name(input));
            if tokio::fs::copy(input, &out_path).await.is_ok() {
                tally.bytes_out += in_size;
                // verbatim copy: image bytes out == image bytes in
                let image = report
                    .as_ref()
                    .map(|r| r.main.image_data_bytes())
                    .filter(|&b| b > 0)
                    .unwrap_or(in_size);
                tally.image_bytes_in += image;
                tally.image_bytes_out += image;
                tally.skipped += 1;
            } else {
                tally.failed += 1;
            }
            tally.done += 1;
            tally.current_file = file_name(input);
            tally.current_path = input.to_string_lossy().to_string();
        } else {
            to_convert.push((input.clone(), in_size, report));
        }
    }

    if !to_convert.is_empty() && !token.is_cancelled() {
        let mut args = args_prefix;
        args.push("-d".to_string());
        args.push(plan.out_folder.to_string_lossy().to_string());
        for (input, _, _) in &to_convert {
            args.push(input.to_string_lossy().to_string());
        }
        run_converter(&engine, &args, &token).await;
    }

    if token.is_cancelled() {
        return tally;
    }

    // Tally results. Adobe DNG Converter fails silently, so verify each
    // output exists and parses clean; on any problem, pass the original
    // through so the sequence stays complete.
    for (input, in_size, input_report) in to_convert {
        let out_path = plan.out_folder.join(file_name(&input));
        let out_size = file_size(&out_path).await;
        let image_in = input_report
            .as_ref()
            .map(|r| r.main.image_data_bytes())
            .filter(|&b| b > 0)
            .unwrap_or(in_size);
        tally.image_bytes_in += image_in;

        let mut ok = out_size > 0;
        let mut image_out = out_size;
        if ok {
            match tiff::inspect(&out_path) {
                Ok(report) => {
                    let problems = report.main.verify(report.file_size);
                    if problems.is_empty() {
                        image_out = report.main.image_data_bytes();
                        if image_out == 0 {
                            image_out = out_size;
                        }
                    } else {
                        eprintln!(
                            "[dng] output failed verification ({}): {}",
                            out_path.display(),
                            problems.join("; ")
                        );
                        ok = false;
                    }
                }
                Err(e) => {
                    eprintln!("[dng] could not parse output {}: {e}", out_path.display());
                    ok = false;
                }
            }
        }

        if ok {
            tally.bytes_out += out_size;
            tally.image_bytes_out += image_out;
        } else {
            if tokio::fs::copy(&input, &out_path).await.is_ok() {
                tally.bytes_out += in_size;
                tally.image_bytes_out += image_in;
            }
            tally.failed += 1;
        }
        tally.done += 1;
        tally.current_file = file_name(&input);
        tally.current_path = input.to_string_lossy().to_string();
    }

    tally
}

/// Run a compression job, streaming progress via `send_chunk`. Cancellation is
/// driven by `token` (the frontend calls `cancel_job`).
pub async fn compress(
    source_dir: &str,
    dest_dir: &str,
    mode: &str,
    recursive: bool,
    workers_requested: usize,
    token: &CancellationToken,
    mut send_chunk: impl FnMut(ProgressChunk) + Send + 'static,
) -> Result<JobResult, String> {
    let start = Instant::now();
    let engine = find_engine().ok_or_else(|| "Adobe DNG Converter is not installed.".to_string())?;
    let mode_args = mode_args(mode).ok_or_else(|| "Invalid compression mode".to_string())?;
    let workers = resolve_workers(workers_requested);
    // Skip-if-compressed applies to default/lossless; lossy mode keeps
    // processing everything until its markers are understood and tested.
    let allow_skip = mode != "high";

    let source = std::path::absolute(source_dir).map_err(|e| e.to_string())?;
    let dest = std::path::absolute(dest_dir).map_err(|e| e.to_string())?;

    let folders = scan_folders(&source, recursive).await?;
    let total_files: u64 = folders.iter().map(|f| f.dng_files.len() as u64).sum();

    // Plan every batch up front and create output folders serially (cheap).
    let mut plans: Vec<BatchPlan> = Vec::new();
    for folder in &folders {
        let out_folder = dest.join(&folder.rel_dir);
        tokio::fs::create_dir_all(&out_folder)
            .await
            .map_err(|e| e.to_string())?;
        for batch in chunk(&folder.dng_files, BATCH_SIZE) {
            plans.push(BatchPlan {
                out_folder: out_folder.clone(),
                inputs: batch,
            });
        }
    }

    send_chunk(progress_chunk(
        0,
        total_files,
        String::new(),
        String::new(),
        0,
        0,
        0,
        0,
    ));

    // Progress aggregator: batch workers report tallies over a channel; one
    // task owns `send_chunk` and emits at most every 250 ms.
    let (tally_tx, tally_rx) = mpsc::unbounded_channel::<BatchTally>();
    let agg_token = token.clone();
    let aggregator = tokio::spawn(async move {
        let mut files_done: u64 = 0;
        let mut files_failed: u64 = 0;
        let mut files_skipped: u64 = 0;
        let mut bytes_in: u64 = 0;
        let mut bytes_out: u64 = 0;
        let mut image_bytes_in: u64 = 0;
        let mut image_bytes_out: u64 = 0;
        let mut current_file = String::new();
        let mut current_path = String::new();
        let mut interval = tokio::time::interval(Duration::from_millis(250));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut tally_rx = tally_rx;
        loop {
            tokio::select! {
                tally = tally_rx.recv() => {
                    match tally {
                        Some(t) => {
                            files_done += t.done;
                            files_failed += t.failed;
                            files_skipped += t.skipped;
                            bytes_in += t.bytes_in;
                            bytes_out += t.bytes_out;
                            image_bytes_in += t.image_bytes_in;
                            image_bytes_out += t.image_bytes_out;
                            if !t.current_file.is_empty() {
                                current_file = t.current_file;
                                current_path = t.current_path;
                            }
                        }
                        None => break,
                    }
                }
                _ = interval.tick() => {}
                _ = agg_token.cancelled() => {
                    // drain anything left, then finish
                    while let Ok(t) = tally_rx.try_recv() {
                        files_done += t.done;
                        files_failed += t.failed;
                        files_skipped += t.skipped;
                        bytes_in += t.bytes_in;
                        bytes_out += t.bytes_out;
                        image_bytes_in += t.image_bytes_in;
                        image_bytes_out += t.image_bytes_out;
                        if !t.current_file.is_empty() {
                            current_file = t.current_file;
                            current_path = t.current_path;
                        }
                    }
                    break;
                }
            }
            send_chunk(progress_chunk(
                files_done,
                total_files,
                current_file.clone(),
                current_path.clone(),
                bytes_in,
                bytes_out,
                files_failed,
                files_skipped,
            ));
        }
        send_chunk(progress_chunk(
            files_done,
            total_files,
            current_file,
            current_path,
            bytes_in,
            bytes_out,
            files_failed,
            files_skipped,
        ));
        (
            files_done,
            files_failed,
            files_skipped,
            bytes_in,
            bytes_out,
            image_bytes_in,
            image_bytes_out,
        )
    });

    // Bounded parallel batches. Cancellation is checked before each batch and
    // inside run_converter, which kills the child process on cancel.
    let semaphore = Arc::new(Semaphore::new(workers));
    let args_prefix: Vec<String> = mode_args.iter().map(|s| s.to_string()).collect();
    let mut batch_tasks = Vec::with_capacity(plans.len());
    for plan in plans {
        if token.is_cancelled() {
            break;
        }
        let permit_sem = semaphore.clone();
        let tx = tally_tx.clone();
        let eng = engine.clone();
        let ap = args_prefix.clone();
        let tok = token.clone();
        batch_tasks.push(tokio::spawn(async move {
            let _permit = permit_sem.acquire_owned().await;
            let tally = run_batch(eng, ap, plan, allow_skip, tok).await;
            let _ = tx.send(tally);
        }));
    }
    drop(tally_tx);

    for task in batch_tasks {
        let _ = task.await;
    }
    let (files_done, files_failed, files_skipped, bytes_in, bytes_out, image_bytes_in, image_bytes_out) =
        aggregator.await.unwrap_or((0, 0, 0, 0, 0, 0, 0));

    // Copy non-DNG companion files (audio, metadata) so clips stay intact.
    let mut files_copied: u64 = 0;
    if !token.is_cancelled() {
        for folder in &folders {
            let out_folder = dest.join(&folder.rel_dir);
            for other in &folder.other_files {
                if token.is_cancelled() {
                    break;
                }
                let dest_path = out_folder.join(file_name(other));
                if tokio::fs::copy(other, &dest_path).await.is_ok() {
                    files_copied += 1;
                }
            }
        }
    }

    let ratio = if bytes_out > 0 {
        bytes_in as f64 / bytes_out as f64
    } else {
        1.0
    };
    // The honest ratio: image data in vs image data out (TileByteCounts,
    // falling back to StripByteCounts, then to whole-file size).
    let true_ratio = if image_bytes_out > 0 {
        image_bytes_in as f64 / image_bytes_out as f64
    } else {
        1.0
    };
    Ok(JobResult {
        files_processed: files_done,
        files_failed,
        files_copied,
        files_skipped,
        bytes_in,
        bytes_out,
        ratio,
        image_bytes_in,
        image_bytes_out,
        true_ratio,
        elapsed_ms: start.elapsed().as_millis() as u64,
        output_dir: dest.to_string_lossy().to_string(),
        canceled: token.is_cancelled(),
    })
}

// ── Unit tests ───────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_dng_matches_case_insensitively() {
        assert!(is_dng("frame0001.dng"));
        assert!(is_dng("FRAME0001.DNG"));
        assert!(is_dng("clip.Dng"));
        assert!(is_dng(".dng"));
        assert!(!is_dng("frame.dng.bak"));
        assert!(!is_dng("adng"));
        assert!(!is_dng("frame.dng "));
        assert!(!is_dng("wav"));
        assert!(!is_dng(""));
    }

    #[test]
    fn resolve_workers_auto_uses_half_cores_clamped() {
        let cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        let expected = (cores / 2).max(2).min(MAX_WORKERS);
        assert_eq!(resolve_workers(0), expected);
        assert!(resolve_workers(0) >= 2);
        assert!(resolve_workers(0) <= MAX_WORKERS);
    }

    #[test]
    fn resolve_workers_clamps_explicit_values() {
        assert_eq!(resolve_workers(1), 1);
        assert_eq!(resolve_workers(4), 4);
        assert_eq!(resolve_workers(8), 8);
        assert_eq!(resolve_workers(99), MAX_WORKERS);
    }

    #[test]
    fn chunk_splits_into_batches() {
        let v: Vec<i32> = (0..35).collect();
        let chunks = chunk(&v, 16);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].len(), 16);
        assert_eq!(chunks[1].len(), 16);
        assert_eq!(chunks[2].len(), 3);
        assert_eq!(chunks[2], vec![32, 33, 34]);

        let empty: Vec<i32> = vec![];
        assert!(chunk(&empty, 16).is_empty());

        let small = vec![1, 2];
        let chunks = chunk(&small, 16);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], vec![1, 2]);
    }

    #[test]
    fn mode_args_match_original_flags() {
        assert_eq!(mode_args("default"), Some(&["-c", "-p1", "-fl"][..]));
        assert_eq!(mode_args("lossless"), Some(&["-c", "-p0"][..]));
        assert_eq!(mode_args("high"), Some(&["-lossy", "-p1", "-fl"][..]));
        assert_eq!(mode_args("bogus"), None);
        assert_eq!(mode_args(""), None);
    }

    #[test]
    fn transient_detector_catches_download_drops() {
        assert!(is_transient_download_error(
            "curl: (18) transfer closed with 12345 bytes remaining to read (partial file)"
        ));
        assert!(is_transient_download_error("curl: (28) Operation timed out"));
        assert!(is_transient_download_error("Connection reset by peer"));
        assert!(is_transient_download_error("TIMED OUT after 60000ms"));
        assert!(!is_transient_download_error(
            "Error: No available formula with the name \"adobe-dng-converter\""
        ));
    }

    #[test]
    fn privileged_detector_catches_sudo_installer_failures() {
        assert!(is_privileged_installer_error(
            "sudo: a password is required\ninstaller -pkg failed"
        ));
        assert!(is_privileged_installer_error(
            "Failure while executing installer tool"
        ));
        assert!(!is_privileged_installer_error("curl: (18) partial file"));
    }

    #[test]
    fn summarize_prefers_privileged_over_everything() {
        let raw = "Error: Failure while executing; sudo installer -pkg \"/tmp/x.pkg\" failed\ncurl: (18) partial file";
        assert_eq!(
            summarize_install_error(raw),
            "This installer needs your Mac password to finish, which we can't ask for in the background. Use the manual download below — it'll prompt you the normal way."
        );
    }

    #[test]
    fn summarize_reports_transient_after_retry() {
        let raw = "==> Downloading https://example.com/x.dmg\ncurl: (18) transfer closed (partial file)";
        assert_eq!(
            summarize_install_error(raw),
            "Adobe's download was interrupted partway through, even after a retry. This usually clears up on its own — try again in a moment, or use the manual download below."
        );
    }

    #[test]
    fn summarize_picks_last_error_line_and_truncates() {
        let raw = "==> Installing\nError: first problem happened here\nmore noise\nError: second problem happened here";
        assert_eq!(
            summarize_install_error(raw),
            "Error: second problem happened here"
        );

        let long_line = format!("Error: {}", "x".repeat(300));
        let summarized = summarize_install_error(&long_line);
        assert_eq!(summarized.chars().count(), 221); // 220 chars + …
        assert!(summarized.ends_with('…'));
        assert!(summarized.starts_with("Error: xxx"));
    }

    #[test]
    fn summarize_falls_back_to_generic_message() {
        assert_eq!(
            summarize_install_error("some unrecognized output"),
            "Homebrew couldn't install it. Try again, or use the manual download below."
        );
        assert_eq!(
            summarize_install_error(""),
            "Homebrew couldn't install it. Try again, or use the manual download below."
        );
    }
}
