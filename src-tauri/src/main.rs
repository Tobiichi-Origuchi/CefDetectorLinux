#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod cli;
mod icon_finder;
pub mod models;
pub mod package_manager;
pub mod search;

use icon_finder::get_app_icon;
use search::{open_path, start_search};

fn main() {
    cli::handle_cli();
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            use tauri::Manager;
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .invoke_handler(tauri::generate_handler![
            get_app_icon,
            start_search,
            open_path
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
