// src-tauri/src/lib.rs
//
// App wiring: modules, managed state, command registration, shutdown.

mod analytics;
mod commands;
mod connections;
mod folder;
mod import;
mod names;
mod query;
mod query_engine;
mod staging;
mod workspace;

pub use connections::ConnectionManager;
pub use query_engine::{CountCache, QueryModel, QueryResult};
pub use workspace::{Workspace, WorkspaceStore};

use tauri::{Manager, RunEvent};

/// Workspaces live in ~/workspaces (C:\Users\<you>\workspaces on Windows).
fn workspaces_root(app: &tauri::App) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    Ok(app.path().home_dir()?.join("workspaces"))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let store = WorkspaceStore::new(workspaces_root(app)?)?;
            app.manage(store);
            app.manage(ConnectionManager::new());
            app.manage(CountCache::default());
            app.manage(commands::ImportControl::default());
            app.manage(analytics::AnalyticsState::default());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // workspaces
            commands::create_workspace,
            commands::list_workspaces,
            commands::get_workspace,
            commands::delete_workspace,
            // folders
            commands::create_folder,
            commands::list_folders,
            commands::get_folder,
            commands::delete_folder,
            // tables & queries
            commands::list_tables,
            commands::table_meta,
            commands::execute_query,
            // import
            commands::list_sheets,
            commands::import_file_to_folder,
            commands::cancel_import,
            // staged import (review columns before creating the table)
            commands::stage_import,
            commands::check_import_column,
            commands::commit_import,
            commands::discard_import,
            // ACL commands
            commands::run_command,
            commands::list_results,
            commands::save_result,
            commands::delete_result,
            commands::command_history,
            commands::get_variables,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|handle, event| {
        if let RunEvent::Exit = event {
            // Flush and close every open DuckDB file.
            handle.state::<ConnectionManager>().close_all();
        }
    });
}
