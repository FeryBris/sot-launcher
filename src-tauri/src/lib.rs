mod commands;
mod settings;
mod sync;
mod types;

use commands::AppState;
use tauri::{Emitter, Manager};
use tauri_plugin_deep_link::DeepLinkExt;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  tauri::Builder::default()
    // Must be registered first: on Windows/Linux a deep link relaunches the
    // app as a brand new process, so without this every sot-launcher://
    // click would open a second window instead of reaching the running one.
    // The "deep-link" cargo feature makes this automatically forward the
    // second launch's URL into the deep-link plugin below.
    .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
      if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_focus();
      }
    }))
    .manage(AppState::default())
    .setup(|app| {
      if cfg!(debug_assertions) {
        app.handle().plugin(
          tauri_plugin_log::Builder::default()
            .level(log::LevelFilter::Info)
            .build(),
        )?;
      }

      // On Windows/Linux the OS only knows to route sot-launcher:// links
      // here if something has told it to — installers do this on a real
      // install, but an unbundled `tauri dev` binary is never registered
      // that way, so without this the link silently goes nowhere (no
      // error, since the OS never hands it to the app at all).
      if let Err(e) = app.deep_link().register_all() {
        log::warn!("failed to register deep link scheme: {e}");
      }

      let handle = app.handle().clone();
      app.deep_link().on_open_url(move |event| {
        let Some(url) = event.urls().first().cloned() else { return };
        let Some(code) = url.query_pairs().find(|(k, _)| k == "code").map(|(_, v)| v.into_owned()) else { return };

        let handle = handle.clone();
        tauri::async_runtime::spawn(async move {
          let http = handle.state::<AppState>().http.clone();
          match commands::exchange_pair_code(&handle, &http, code).await {
            Ok(settings) => {
              let _ = handle.emit("discord-paired", &settings);
            }
            Err(e) => {
              let _ = handle.emit("discord-pair-error", &e);
            }
          }
        });
      });

      Ok(())
    })
    .plugin(tauri_plugin_dialog::init())
    .plugin(tauri_plugin_shell::init())
    .plugin(tauri_plugin_updater::Builder::new().build())
    .plugin(tauri_plugin_process::init())
    .plugin(tauri_plugin_deep_link::init())
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
      commands::get_installed_state,
      commands::set_addon_disabled,
      commands::delete_addon,
      commands::run_sync,
      commands::launch_game,
      commands::pair_with_code,
      commands::clear_discord_login,
    ])
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
