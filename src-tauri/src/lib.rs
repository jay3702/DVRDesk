/// Read a local file and return its contents as a UTF-8 string.
/// Used to load SRT subtitle sidecar files from a network share.
///
/// Non-async #[tauri::command] handlers run on the main thread, so a plain
/// std::fs::read_to_string here would block the entire UI (window repaints,
/// clicks, everything) for as long as an unreachable network share hangs.
/// Run the read on a blocking thread and bound it with a timeout so a stale
/// or unreachable share fails fast instead of freezing the app.
#[tauri::command]
async fn read_text_file(path: String) -> Result<String, String> {
    let path_for_err = path.clone();
    let read = tauri::async_runtime::spawn_blocking(move || std::fs::read_to_string(&path));

    match tokio::time::timeout(std::time::Duration::from_secs(10), read).await {
        Ok(Ok(Ok(contents))) => Ok(contents),
        Ok(Ok(Err(e))) => Err(format!("{}: {}", path_for_err, e)),
        Ok(Err(e)) => Err(format!("{}: task error: {}", path_for_err, e)),
        Err(_) => Err(format!("{}: timed out after 10s (network share unreachable?)", path_for_err)),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![read_text_file])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
