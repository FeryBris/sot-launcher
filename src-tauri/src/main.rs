// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
  // Works around a blank/white webview on Linux, seen with newer
  // webkit2gtk (2.42+) builds where the DMA-BUF renderer misbehaves on
  // some GPU/driver combos — most visible on Arch, which ships a much
  // newer webkit2gtk-4.1 than the Ubuntu 22.04 build environment ever
  // tests against. Must be set before the webview is created, and this is
  // the only thread running at that point, so the data race env::set_var
  // normally guards against can't happen here.
  #[cfg(target_os = "linux")]
  unsafe {
    std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
  }

  app_lib::run();
}
