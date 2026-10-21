mod dng;
mod jobs;
mod tiff;

use jobs::JobState;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_dialog::{DialogExt, FilePath};
use tauri_plugin_opener::OpenerExt;

const ADOBE_DOWNLOAD_URL: &str = "https://helpx.adobe.com/camera-raw/using/adobe-dng-converter.html";

#[tauri::command]
fn check_engine() -> Result<dng::EngineStatus, String> {
    Ok(dng::check_engine())
}

#[tauri::command]
async fn install_engine() -> Result<dng::InstallResult, String> {
    Ok(dng::install_engine().await)
}

#[tauri::command]
fn open_download_page(app: AppHandle) -> Result<(), String> {
    app.opener()
        .open_url(ADOBE_DOWNLOAD_URL, None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn scan_folder(source_dir: String, recursive: bool) -> Result<dng::ScanResult, String> {
    if source_dir.trim().is_empty() {
        return Err("Invalid sourceDir".to_string());
    }
    dng::scan(&source_dir, recursive).await
}

#[tauri::command]
fn reveal(app: AppHandle, path: String) -> Result<(), String> {
    app.opener()
        .open_path(path, None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn pick_folder(app: AppHandle, title: String) -> Result<Option<String>, String> {
    let picked = app
        .dialog()
        .file()
        .set_title(title)
        .blocking_pick_folder();
    Ok(picked.map(|p| match p {
        FilePath::Path(path) => path.to_string_lossy().to_string(),
        FilePath::Url(url) => url.to_string(),
    }))
}

#[tauri::command]
async fn run_compress(
    app: AppHandle,
    state: State<'_, JobState>,
    source_dir: String,
    dest_dir: String,
    mode: String,
    recursive: bool,
    workers: u32,
    job_id: String,
) -> Result<dng::JobResult, String> {
    if !["default", "lossless", "high"].contains(&mode.as_str()) {
        return Err("Invalid compression mode".to_string());
    }
    if source_dir.trim().is_empty() {
        return Err("Invalid sourceDir".to_string());
    }
    if dest_dir.trim().is_empty() {
        return Err("Invalid destDir".to_string());
    }

    let token = state.token_for(&job_id);
    let event_name = format!("compress-progress-{job_id}");
    let result = dng::compress(
        &source_dir,
        &dest_dir,
        &mode,
        recursive,
        workers as usize,
        &token,
        move |chunk| {
            let _ = app.emit(&event_name, chunk);
        },
    )
    .await;
    state.remove(&job_id);
    result
}

#[tauri::command]
fn frame_thumbnail(path: String) -> Result<Option<String>, String> {
    if path.trim().is_empty() {
        return Err("Invalid path".to_string());
    }
    Ok(tiff::thumbnail_data_url(std::path::Path::new(&path)))
}

#[tauri::command]
fn cancel_job(job_id: String, state: State<'_, JobState>) {
    state.cancel(&job_id);
}

/// One-shot compress for `cmprssr --selftest <source> [dest] [mode]`. Runs the
/// real engine on a folder and prints the tallies as JSON so the native binary
/// can be proven on real footage without launching the Tauri window. Exit code
/// 0 on success, 1 on engine failure, 2 on bad arguments.
fn run_selftest(src: &str, dst: &str, mode: &str, recursive: bool) -> i32 {
    if !["default", "lossless", "high"].contains(&mode) {
        eprintln!("invalid mode {mode:?} (want default|lossless|high)");
        return 2;
    }
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("runtime build failed: {e}");
            return 2;
        }
    };
    let token = tokio_util::sync::CancellationToken::new();
    // workers = 0 (Auto) — mirror the GUI's default. `recursive` is passed in.
    let result = rt.block_on(dng::compress(
        src, dst, mode, recursive, 0, &token, |_chunk| {},
    ));
    match result {
        Ok(r) => {
            println!("{{");
            println!("  \"source\": {:?},", src);
            println!("  \"dest\": {:?},", dst);
            println!("  \"mode\": {:?},", mode);
            println!("  \"filesProcessed\": {},", r.files_processed);
            println!("  \"filesFailed\": {},", r.files_failed);
            println!("  \"filesSkipped\": {},", r.files_skipped);
            println!("  \"filesCopied\": {},", r.files_copied);
            println!("  \"bytesIn\": {},", r.bytes_in);
            println!("  \"bytesOut\": {},", r.bytes_out);
            println!("  \"ratio\": {},", r.ratio);
            println!("  \"imageBytesIn\": {},", r.image_bytes_in);
            println!("  \"imageBytesOut\": {},", r.image_bytes_out);
            println!("  \"trueRatio\": {},", r.true_ratio);
            println!("  \"elapsedMs\": {},", r.elapsed_ms);
            println!("  \"canceled\": {}", r.canceled);
            println!("}}");
            0
        }
        Err(e) => {
            eprintln!("selftest failed: {e}");
            1
        }
    }
}

fn main() {
    // CLI self-test: `cmprssr --selftest <source_dir> [dest_dir] [mode]` runs
    // one compress end-to-end against the real Adobe engine and prints the
    // tallies, then exits — no GUI. Everything else is the Tauri app.
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() == Some("--selftest") {
        let src = args.next().unwrap_or_else(|| ".".to_string());
        let dst = args.next().unwrap_or_else(|| format!("{src}.selftest-out"));
        let mode = args.next().unwrap_or_else(|| "default".to_string());
        // optional 4th arg: recursive (1/true) vs top-level only (0/false, default)
        let recursive = matches!(args.next().as_deref(), Some("1") | Some("true"));
        std::process::exit(run_selftest(&src, &dst, &mode, recursive));
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(JobState::new())
        .invoke_handler(tauri::generate_handler![
            check_engine,
            install_engine,
            open_download_page,
            scan_folder,
            reveal,
            pick_folder,
            run_compress,
            cancel_job,
            frame_thumbnail
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
