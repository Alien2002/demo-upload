// src-tauri/src/lib.rs
// Replaces the template's lib.rs (the `greet` example is no longer needed).

mod commands;
mod import;
mod query;
mod state;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // One workspace database in the app's data folder
            // (on Windows: %APPDATA%\<your.app.identifier>\workspace.duckdb)
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let db = state::Db::open(&dir.join("workspace.duckdb"))?;
            app.manage(db);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::import_file,
            commands::list_tables,
            commands::table_meta,
            commands::fetch_rows,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
