/** Best-effort OS detection for the webview itself — same technique used
 *  to default the Settings page's platform tab. Only used to pick which
 *  of Settings.gamePathWindows/gamePathLinux is "the real one" for this
 *  running instance; not a substitute for Rust's cfg!(target_os). */
export function isWindows(): boolean {
  return !navigator.userAgent.toLowerCase().includes('linux')
}
