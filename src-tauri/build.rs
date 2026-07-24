use std::fs;
use std::path::Path;

/// Bakes `SOT_API_KEY`/`SOT_API_BASE_URL` from `.env` into the binary at
/// compile time via `env!()`, so they're never part of user-editable
/// settings.json and never shown in the UI. `.env` is gitignored — copy
/// `.env.example` to `.env` and fill in the real values before building.
fn load_dotenv() {
  let path = Path::new(".env");
  println!("cargo:rerun-if-changed=.env");

  let Ok(contents) = fs::read_to_string(path) else {
    return;
  };

  for line in contents.lines() {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
      continue;
    }
    let Some((key, value)) = line.split_once('=') else {
      continue;
    };
    let key = key.trim();
    let value = value.trim().trim_matches('"');
    println!("cargo:rustc-env={key}={value}");
  }
}

fn main() {
  load_dotenv();
  tauri_build::build()
}
