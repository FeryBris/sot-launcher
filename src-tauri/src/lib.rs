mod commands;
mod settings;
mod sync;
mod types;

use commands::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  tauri::Builder::default()
    .manage(AppState::default())
    .setup(|app| {
      if cfg!(debug_assertions) {
        app.handle().plugin(
          tauri_plugin_log::Builder::default()
            .level(log::LevelFilter::Info)
            .build(),
        )?;
      }
      Ok(())
    })
    .plugin(tauri_plugin_dialog::init())
    .plugin(tauri_plugin_shell::init())
    .plugin(tauri_plugin_updater::Builder::new().build())
    .plugin(tauri_plugin_process::init())
    .invoke_handler(tauri::generate_handler![
      commands::get_settings,
      commands::set_game_path,
      commands::set_api_base_url,
      commands::set_api_key,
      commands::set_launch_command,
      commands::set_close_on_launch,
      commands::set_auto_launch,
      commands::pick_game_folder,
      commands::fetch_manifest,
      commands::get_lock_state,
      commands::run_sync,
      commands::launch_game,
    ])
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
