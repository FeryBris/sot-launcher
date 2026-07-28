// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
  // Works around a blank/white webview on Linux, seen with newer
  // webkit2gtk (2.42+) builds where accelerated compositing misbehaves on
  // some GPU/driver combos — confirmed on a Steam Deck (AMD Van Gogh APU,
  // Mesa RADV, gamescope/Wayland desktop session); disabling DMA-BUF alone
  // (0.1.8) wasn't enough, compositing itself also has to go. Must be set
  // before the webview is created, and this is the only thread running at
  // that point, so the data race env::set_var normally guards against
  // can't happen here.
  #[cfg(target_os = "linux")]
  unsafe {
    std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
  }

  app_lib::run();
}
