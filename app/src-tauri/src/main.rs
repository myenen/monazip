#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use monazip_core::extract::{ExtractionReport, extract_zstd};
use monazip_core::focused::{
    FocusProgress, FocusReport, FocusResult, ZstdTuning, analyze_focused_with_tuning,
};
use serde::Serialize;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tauri::{State, ipc::Channel};

#[derive(Default)]
struct AppState {
    active: Mutex<Option<Arc<AtomicBool>>>,
    result: Mutex<Option<Arc<FocusResult>>>,
}

fn begin_operation(state: &AppState) -> Result<Arc<AtomicBool>, String> {
    let mut active = state.active.lock().unwrap();
    if active.is_some() {
        return Err("Bir işlem zaten çalışıyor.".into());
    }
    let cancel = Arc::new(AtomicBool::new(false));
    *active = Some(Arc::clone(&cancel));
    Ok(cancel)
}

#[derive(Serialize)]
struct FileInfo {
    name: String,
    path: PathBuf,
    size: u64,
}

#[tauri::command]
fn inspect_file(path: PathBuf) -> Result<FileInfo, String> {
    let metadata = fs::metadata(&path).map_err(|error| error.to_string())?;
    if !metadata.is_file() {
        return Err("Okunabilir bir normal dosya seçin.".into());
    }
    let name = path
        .file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned();
    Ok(FileInfo {
        name,
        path,
        size: metadata.len(),
    })
}

#[tauri::command]
async fn start_analysis(
    state: State<'_, AppState>,
    path: PathBuf,
    level: i32,
    tuning: Option<ZstdTuning>,
    progress: Channel<FocusProgress>,
) -> Result<FocusReport, String> {
    let cancel = begin_operation(&state)?;
    *state.result.lock().unwrap() = None;
    let result = tauri::async_runtime::spawn_blocking(move || {
        let mut last = None::<Instant>;
        analyze_focused_with_tuning(
            path,
            level,
            tuning.unwrap_or_default(),
            |update| {
                let now = Instant::now();
                if update.processed_bytes == 0
                    || update.processed_bytes == update.total_bytes
                    || last.is_none_or(|time| now.duration_since(time) >= Duration::from_millis(75))
                {
                    let _ = progress.send(update);
                    last = Some(now);
                }
            },
            &cancel,
        )
    })
    .await;
    let mut active = state.active.lock().unwrap();
    *active = None;
    let result = Arc::new(
        result
            .map_err(|error| error.to_string())?
            .map_err(|error| error.to_string())?,
    );
    let report = result.report.clone();
    *state.result.lock().unwrap() = Some(result);
    Ok(report)
}

#[tauri::command]
fn cancel_operation(state: State<'_, AppState>) {
    if let Some(cancel) = state.active.lock().unwrap().as_ref() {
        cancel.store(true, Ordering::Relaxed);
    }
}

#[tauri::command]
async fn extract_archive(
    state: State<'_, AppState>,
    path: PathBuf,
    destination: PathBuf,
    progress: Channel<u64>,
) -> Result<ExtractionReport, String> {
    let cancel = begin_operation(&state)?;
    let result = tauri::async_runtime::spawn_blocking(move || {
        let mut last = Instant::now();
        extract_zstd(
            path,
            destination,
            |restored| {
                if restored == 0 || last.elapsed() >= Duration::from_millis(75) {
                    let _ = progress.send(restored);
                    last = Instant::now();
                }
            },
            &cancel,
        )
    })
    .await;
    *state.active.lock().unwrap() = None;
    result
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn save_report(state: State<'_, AppState>, path: PathBuf) -> Result<(), String> {
    let result = state
        .result
        .lock()
        .unwrap()
        .clone()
        .ok_or("Kaydedilecek rapor yok.")?;
    tauri::async_runtime::spawn_blocking(move || {
        let dir = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut file = tempfile::NamedTempFile::new_in(dir).map_err(|error| error.to_string())?;
        serde_json::to_writer_pretty(&mut file, &result.report)
            .map_err(|error| error.to_string())?;
        writeln!(file).map_err(|error| error.to_string())?;
        file.persist_noclobber(path).map_err(|error| {
            if error.error.kind() == std::io::ErrorKind::AlreadyExists {
                "Bu dosya zaten var. Rapor için yeni bir ad seçin.".into()
            } else {
                error.to_string()
            }
        })?;
        Ok(())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn save_archive(state: State<'_, AppState>, path: PathBuf) -> Result<(), String> {
    let result = state
        .result
        .lock()
        .unwrap()
        .clone()
        .ok_or("Kaydedilecek arşiv yok.")?;
    tauri::async_runtime::spawn_blocking(move || result.save_archive(&path))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

fn configure<R: tauri::Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    builder
        .manage(AppState::default())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            inspect_file,
            start_analysis,
            cancel_operation,
            extract_archive,
            save_report,
            save_archive
        ])
}

fn main() {
    configure(tauri::Builder::default())
        .run(tauri::generate_context!())
        .expect("MonaZip başlatılamadı");
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use tauri::{
        WebviewWindowBuilder,
        ipc::{CallbackFn, InvokeBody},
        test::{INVOKE_KEY, get_ipc_response, mock_builder, mock_context, noop_assets},
        webview::InvokeRequest,
    };

    #[test]
    fn desktop_ipc_selects_analyzes_and_exports_without_replacing_source() {
        let phases = Arc::new(Mutex::new(Vec::new()));
        let channel_phases = Arc::clone(&phases);
        let app = configure(mock_builder())
            .channel_interceptor(move |_, _, _, body| {
                let update = body.clone().deserialize::<Value>().unwrap();
                channel_phases.lock().unwrap().push(update["stage"].clone());
                true
            })
            .build(mock_context(noop_assets()))
            .unwrap();
        let webview = WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();
        let invoke = |cmd: &str, body: Value| {
            let origin = if cfg!(target_os = "windows") {
                "http://tauri.localhost"
            } else {
                "tauri://localhost"
            };
            get_ipc_response(
                &webview,
                InvokeRequest {
                    cmd: cmd.into(),
                    callback: CallbackFn(0),
                    error: CallbackFn(1),
                    url: origin.parse().unwrap(),
                    body: InvokeBody::Json(body),
                    headers: Default::default(),
                    invoke_key: INVOKE_KEY.into(),
                },
            )
            .map(|body| body.deserialize::<Value>().unwrap())
        };

        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source.txt");
        let dest = directory.path().join("source.report.json");
        let bytes = b"MonaZip".repeat(2500);
        fs::write(&source, &bytes).unwrap();
        let file = invoke("inspect_file", json!({ "path": source })).unwrap();
        assert_eq!(file["size"], bytes.len());
        let analyze_at = |level, tuning| {
            let report = invoke(
                "start_analysis",
                json!({ "path": source, "level": level, "tuning": tuning, "progress": "__CHANNEL__:12" }),
            )
            .unwrap();
            assert_eq!(report["verified"], true);
            assert_eq!(report["monazip"]["level"], level);
            assert_eq!(report["zstd_tuning"], tuning);
            report
        };
        analyze_at(15, "standard");
        analyze_at(19, "standard");
        analyze_at(9, "standard");
        analyze_at(9, "parallel");
        let report = analyze_at(9, "parallel_long");
        let seen = phases.lock().unwrap();
        assert!(seen.contains(&json!("compressing")));
        assert!(seen.contains(&json!("verifying")));
        drop(seen);
        assert!(invoke("save_report", json!({ "path": source })).is_err());
        invoke("save_report", json!({ "path": dest })).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&fs::read(dest).unwrap()).unwrap(),
            report
        );
        let archive = directory.path().join("source.txt.zst");
        invoke("save_archive", json!({ "path": archive })).unwrap();
        assert_eq!(
            fs::metadata(&archive).unwrap().len(),
            report["monazip"]["bytes"].as_u64().unwrap()
        );
        let restored = directory.path().join("restored.txt");
        let extraction = invoke(
            "extract_archive",
            json!({
                "path": archive, "destination": restored, "progress": "__CHANNEL__:13"
            }),
        )
        .unwrap();
        assert_eq!(extraction["restored_bytes"], bytes.len());
        assert_eq!(fs::read(restored).unwrap(), bytes);
        assert!(
            invoke(
                "extract_archive",
                json!({
                    "path": archive, "destination": source, "progress": "__CHANNEL__:14"
                })
            )
            .is_err()
        );
        assert!(invoke("save_archive", json!({ "path": source })).is_err());
        assert_eq!(fs::read(source).unwrap(), bytes);
    }
}
