mod commands;
mod config;
mod guild_roster;
mod guild_sync;
mod settings;
mod sync;
mod sync_log;
mod types;
mod watcher;

use commands::AppState;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager};
use tauri_plugin_deep_link::DeepLinkExt;

/// Brings the main window back, whether it was hidden (via `hide_on_launch`)
/// or just minimized by the user/window manager — `show()` alone doesn't
/// undo the latter.
fn show_main_window(app: &tauri::AppHandle) {
  if let Some(window) = app.get_webview_window("main") {
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
  }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  // Configured once here (not passed from the frontend on each check) so
  // the API key never needs to reach JS at all.
  let updater = tauri_plugin_updater::Builder::new()
    .header("X-API-KEY", config::API_KEY)
    .expect("invalid API key header value")
    .build();

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

      // Starts watching the SavedVariables files for changes (see
      // watcher.rs) instead of the frontend polling on a timer. If no
      // game folder is configured yet, this is a no-op — set_game_path_*
      // starts it once the player actually picks one.
      if let Ok(s) = settings::load_settings(app.handle()) {
        if let Some(path) = s.game_path() {
          watcher::watch_guild_data(app.handle(), std::path::Path::new(&path));
        }
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

      // Label mirrors the frontend's update-available state (see
      // set_tray_primary_label) — click just emits an event and lets the
      // frontend decide whether that means installUpdate() or playNow(),
      // reusing its existing sync-then-launch/update logic rather than
      // duplicating it here.
      let play_i = MenuItem::with_id(app, "primary", "Play", true, None::<&str>)?;
      let show_i = MenuItem::with_id(app, "show", "Open", true, None::<&str>)?;
      let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
      let tray_menu = Menu::with_items(app, &[&play_i, &show_i, &quit_i])?;

      app.state::<AppState>().tray_primary_item.lock().unwrap().replace(play_i);

      TrayIconBuilder::new()
        .icon(app.default_window_icon().unwrap().clone())
        .menu(&tray_menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
          "primary" => {
            let _ = app.emit("tray-primary-clicked", ());
          }
          "show" => show_main_window(app),
          "quit" => app.exit(0),
          _ => {}
        })
        .on_tray_icon_event(|tray, event| {
          if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
            show_main_window(tray.app_handle());
          }
        })
        .build(app)?;

      Ok(())
    })
    .plugin(tauri_plugin_dialog::init())
    .plugin(tauri_plugin_shell::init())
    .plugin(updater)
    .plugin(tauri_plugin_process::init())
    .plugin(tauri_plugin_deep_link::init())
    // Reads the setting fresh on every close (not cached), same as any
    // other command, so a change takes effect without a restart.
    .on_window_event(|window, event| {
      if let tauri::WindowEvent::CloseRequested { api, .. } = event {
        let close_to_tray = settings::load_settings(window.app_handle())
          .map(|s| s.close_to_tray)
          .unwrap_or(true);
        if close_to_tray {
          api.prevent_close();
          let _ = window.hide();
        }
      }
    })
    .invoke_handler(tauri::generate_handler![
      commands::get_settings,
      commands::get_api_base_url,
      commands::set_tray_primary_label,
      commands::set_game_path_windows,
      commands::set_game_path_linux,
      commands::set_launch_command,
      commands::set_hide_on_launch,
      commands::set_close_to_tray,
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
      commands::list_lutris_games,
      commands::get_guild_sync_data,
      commands::sync_guild_data_to_web,
      commands::get_guild_roster,
      commands::sync_guild_roster_to_web,
      commands::get_sync_log,
      commands::get_my_loot,
    ])
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
