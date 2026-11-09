//! Starting a game that Steam runs through Proton (Linux only).
//!
//! Battle.net has no Linux client, so players add it to Steam as a non-Steam
//! game and Proton installs WoW into that shortcut's prefix
//! (`.../steamapps/compatdata/<appid>/pfx`). Running Wow.exe with plain
//! `wine` would use a different prefix (~/.wine) and the game fails, so:
//!
//! - Battle.net not running: Steam starts the shortcut, then once Battle.net
//!   is up it is asked to launch the game (`--exec="launch <code>"`).
//! - Battle.net already running: Steam would only answer "game is already
//!   running", so the running Battle.net is asked directly.
//!
//! Asking means starting a second Battle.net.exe in the same prefix, with the
//! same Proton and Steam runtime as the running one; it hands the command to
//! the running instance over IPC and exits (measured 2026-11-09: `launch WoWF`
//! starts Forever, `launch_uid wowf` only selects it).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tauri::AppHandle;

/// The appid of the Steam game that runs in the Proton prefix holding
/// `game_root`.
///
/// Steam gives a non-Steam shortcut a new random appid when it is edited or
/// re-added, and with it a new, empty prefix - while WoW stays installed in
/// the old one. The fix players use is to replace the new
/// `compatdata/<new appid>` with a symlink to the old folder, so a symlink in
/// `compatdata` pointing at the game's prefix names the shortcut to start.
pub fn compat_appid(game_root: &Path) -> Option<u32> {
    let parts: Vec<_> = game_root.components().collect();
    let i = parts.windows(3).position(|w| {
        w[0].as_os_str() == "compatdata"
            && w[2].as_os_str() == "pfx"
            && w[1].as_os_str().to_str().is_some_and(|s| s.parse::<u32>().is_ok())
    })?;
    let folder_appid: u32 = parts[i + 1].as_os_str().to_str()?.parse().ok()?;
    let compatdata: PathBuf = parts[..=i].iter().collect();
    Some(symlinked_appid(&compatdata, folder_appid).unwrap_or(folder_appid))
}

/// The appid whose `compatdata/<appid>` is a symlink to `compatdata/<target>`.
fn symlinked_appid(compatdata: &Path, target: u32) -> Option<u32> {
    let target_dir = compatdata.join(target.to_string()).canonicalize().ok()?;
    std::fs::read_dir(compatdata).ok()?.flatten().find_map(|e| {
        let appid: u32 = e.file_name().to_str()?.parse().ok()?;
        let is_link = e.file_type().ok()?.is_symlink();
        (is_link && appid != target && e.path().canonicalize().ok()? == target_dir).then_some(appid)
    })
}

/// The `steam://rungameid/<id>` id. Non-Steam shortcuts have the top bit of
/// their 32-bit appid set, and Steam addresses them as
/// `(appid << 32) | 0x02000000`; a real Steam app's run id is its appid.
pub fn run_id(appid: u32) -> u64 {
    if appid & 0x8000_0000 != 0 {
        (u64::from(appid) << 32) | 0x0200_0000
    } else {
        u64::from(appid)
    }
}

/// Battle.net's launch code for the game in `exe_dir`, from the
/// `.flavor.info` Blizzard writes next to Wow.exe. None: not a Battle.net
/// game the launcher knows how to start (Steam then only opens Battle.net).
pub fn battlenet_launch_code(exe_dir: &Path) -> Option<&'static str> {
    let flavor = std::fs::read_to_string(exe_dir.join(".flavor.info")).ok()?;
    flavor.contains("wowf").then_some("WoWF")
}

/// Starts the game in Steam app `appid`'s prefix - see the module docs.
pub fn launch(app: &AppHandle, appid: u32, launch_code: Option<&'static str>) -> Result<(), String> {
    if let Some(code) = launch_code {
        if let Some(bnet) = find_running_battlenet(appid) {
            return ask_battlenet_to_launch(&bnet, code);
        }
    }

    open_in_steam(run_id(appid))?;
    if let Some(code) = launch_code {
        let app = app.clone();
        std::thread::spawn(move || launch_once_battlenet_is_up(&app, appid, code));
    }
    Ok(())
}

/// `steam` hands the URL to the running Steam client; when it isn't on PATH
/// (Flatpak Steam), the desktop's steam:// handler does the same.
fn open_in_steam(run_id: u64) -> Result<(), String> {
    let url = format!("steam://rungameid/{run_id}");
    std::process::Command::new("steam")
        .arg(&url)
        .spawn()
        .or_else(|_| std::process::Command::new("xdg-open").arg(&url).spawn())
        .map(|_| ())
        .map_err(|e| {
            format!(
                "failed to start the game through Steam ({e}). Make sure Steam is installed, or set \
                 a Custom Launch Command in Settings."
            )
        })
}

/// Waits for the Battle.net Steam just started, then asks it to launch the
/// game - again every 15 s until the game runs, since a Battle.net still
/// starting up (or waiting for sign-in) can drop the request.
fn launch_once_battlenet_is_up(app: &AppHandle, appid: u32, code: &str) {
    const GIVE_UP_AFTER: Duration = Duration::from_secs(180);
    const SETTLE: Duration = Duration::from_secs(5);
    const RETRY: Duration = Duration::from_secs(15);

    let deadline = Instant::now() + GIVE_UP_AFTER;
    let mut next_ask: Option<Instant> = None;
    while Instant::now() < deadline {
        if game_running(appid) {
            crate::debug_log::verbose(app, "launch_game", "game started through Battle.net");
            return;
        }
        if let Some(bnet) = find_running_battlenet(appid) {
            let now = Instant::now();
            match next_ask {
                None => next_ask = Some(now + SETTLE),
                Some(at) if now >= at => {
                    if let Err(e) = ask_battlenet_to_launch(&bnet, code) {
                        crate::debug_log::error(app, "launch_game", &e);
                        return;
                    }
                    next_ask = Some(now + RETRY);
                }
                Some(_) => {}
            }
        }
        std::thread::sleep(Duration::from_secs(2));
    }
    crate::debug_log::warn(
        app,
        "launch_game",
        "Battle.net did not start the game within 3 minutes - press Play in Battle.net",
    );
}

/// A running Battle.net in a Steam app's prefix, and how it was started.
struct RunningBattleNet {
    prefix: String,
    /// Proton's `files` folder (holds bin/wine).
    proton_files: PathBuf,
    /// Steam Linux Runtime's `run` script; Proton's Wine needs a newer glibc
    /// than many hosts have, so it runs inside that container like Steam does.
    runtime_run: Option<PathBuf>,
}

fn find_running_battlenet(appid: u32) -> Option<RunningBattleNet> {
    steam_processes(appid).find_map(|(cmdline, env)| {
        // The main process, not one of its CEF helpers (--type=gpu-process, ...).
        if !cmdline.contains("Battle.net.exe") || cmdline.contains("--type=") {
            return None;
        }
        let prefix = env_var(&env, "WINEPREFIX")?.to_string();
        let tools = env_var(&env, "STEAM_COMPAT_TOOL_PATHS")?;
        let tools: Vec<&Path> = tools.split(':').map(Path::new).collect();
        let proton = tools.iter().find(|p| p.join("proton").is_file())?;
        let runtime_run = tools
            .iter()
            .map(|p| p.join("run"))
            .find(|p| p.is_file() && p.parent().is_some_and(|d| d.join("_v2-entry-point").exists()));
        Some(RunningBattleNet {
            prefix,
            proton_files: proton.join("files"),
            runtime_run,
        })
    })
}

fn game_running(appid: u32) -> bool {
    steam_processes(appid).any(|(cmdline, _)| cmdline.contains("Wow.exe"))
}

fn ask_battlenet_to_launch(bnet: &RunningBattleNet, code: &str) -> Result<(), String> {
    let wine = bnet.proton_files.join("bin/wine");
    let mut cmd = match &bnet.runtime_run {
        Some(run) => {
            let mut c = std::process::Command::new(run);
            c.arg("--").arg(&wine);
            c
        }
        None => std::process::Command::new(&wine),
    };
    cmd.arg(r"C:\Program Files (x86)\Battle.net\Battle.net.exe")
        .arg(format!("--exec=launch {code}"))
        .env("WINEPREFIX", &bnet.prefix)
        .env("WINEDEBUG", "-all")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("failed to ask Battle.net to start the game: {e}"))
}

/// (command line, environment) of every process Steam runs for `appid`.
fn steam_processes(appid: u32) -> impl Iterator<Item = (String, Vec<u8>)> {
    let appid = appid.to_string();
    std::fs::read_dir("/proc")
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().bytes().all(|b| b.is_ascii_digit()))
        .filter_map(move |e| {
            let dir = e.path();
            let env = std::fs::read(dir.join("environ")).ok()?;
            if env_var(&env, "SteamAppId") != Some(appid.as_str()) {
                return None;
            }
            let cmdline = std::fs::read(dir.join("cmdline")).ok()?;
            Some((String::from_utf8_lossy(&cmdline).replace('\0', " "), env))
        })
}

fn env_var<'a>(environ: &'a [u8], name: &str) -> Option<&'a str> {
    environ.split(|&b| b == 0).find_map(|entry| {
        let entry = std::str::from_utf8(entry).ok()?;
        entry.strip_prefix(name)?.strip_prefix('=')
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_non_steam_shortcut_prefix_gets_the_shortcut_run_id() {
        let path = Path::new(
            "/home/u/.steam/steam/steamapps/compatdata/3613542894/pfx/drive_c/Program Files (x86)/World of Warcraft/_forever_",
        );
        assert_eq!(compat_appid(path), Some(3613542894));
        assert_eq!(run_id(3613542894), 15520048552456749056);
    }

    #[test]
    fn a_steam_app_prefix_gets_its_appid() {
        let path = Path::new("/home/u/.steam/steam/steamapps/compatdata/1203620/pfx/drive_c/Game");
        assert_eq!(compat_appid(path), Some(1203620));
        assert_eq!(run_id(1203620), 1203620);
    }

    #[test]
    fn a_plain_wine_or_lutris_folder_is_not_steam() {
        assert_eq!(compat_appid(Path::new("/home/u/Games/wow/drive_c/World of Warcraft")), None);
        assert_eq!(compat_appid(Path::new("/home/u/compatdata/notanid/pfx/wow")), None);
    }

    #[cfg(unix)]
    #[test]
    fn a_shortcut_whose_prefix_is_symlinked_to_the_game_prefix_is_the_one_started() {
        let root = std::env::temp_dir().join(format!("sot-compat-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let compatdata = root.join("steamapps/compatdata");
        let game = compatdata.join("3613542894/pfx/drive_c/World of Warcraft/_forever_");
        std::fs::create_dir_all(&game).unwrap();
        std::fs::create_dir_all(compatdata.join("1203620/pfx")).unwrap();
        std::os::unix::fs::symlink(compatdata.join("3613542894"), compatdata.join("2596161539")).unwrap();

        let appid = compat_appid(&game);
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(appid, Some(2596161539));
    }

    #[test]
    fn env_var_reads_exact_names_only() {
        let env = b"SteamAppIdX=1\0SteamAppId=3613542894\0WINEPREFIX=/p/pfx/\0";
        assert_eq!(env_var(env, "SteamAppId"), Some("3613542894"));
        assert_eq!(env_var(env, "WINEPREFIX"), Some("/p/pfx/"));
        assert_eq!(env_var(env, "Missing"), None);
    }
}
