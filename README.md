# SoT Guild Launcher

Desktop launcher for the guild: syncs mandatory/recommended/optional addons
and WTF/SavedVariables files from the guild server, self-updates, and
launches WoW.exe. Built with [Tauri](https://tauri.app) (Rust + Vue 3 +
TypeScript). The `backend/` folder is a separate Node/Express service that
hosts the manifest, guild news, and launcher releases behind an admin panel.

## Layout

- `src/`, `src-tauri/` — the desktop client (this root project)
- `backend/` — manifest API + admin panel (see `backend/README.md`)

## Client: local development

Requires Rust, Node, and pnpm. On Linux, Tauri also needs system webkit2gtk/
GTK dev packages installed once (see the
[Tauri prerequisites docs](https://v2.tauri.app/start/prerequisites/)).

```bash
pnpm install
pnpm tauri dev
```

Point the launcher at your backend from Settings, or set the default in
`src-tauri/src/types.rs` (`default_api_base_url`).

## Client: building a release

```bash
pnpm tauri build
```

Produces a signed installer (`.msi`/`.exe` on Windows) plus a `.sig` file,
using the keypair in `.tauri-keys/` (see below). Upload both through the
backend admin panel to ship a self-update.

## Self-update signing key

`.tauri-keys/launcher.key` (private, gitignored) and
`.tauri-keys/launcher.key.pub` were generated with `pnpm tauri signer
generate`. The public key is embedded in `src-tauri/tauri.conf.json`. If you
lose the private key, existing installs can no longer verify — and thus
can't install — future updates, so back it up somewhere safe outside this
repo.

## Architecture

- **Client** (Rust): fetches the manifest, downloads addon zips/WTF files,
  verifies each against its published `sha256` before writing anything to
  disk, extracts addons into `Interface/AddOns`, places WTF files at their
  exact target path, and launches `WoW.exe`. Progress streams to the UI via
  Tauri events.
- **Client** (Vue): Home (guild news), Addons (mandatory/recommended/
  optional with sync status), Settings (game folder, server URL), and a
  persistent Play/Sync bar with self-update banner.
- **Backend**: SQLite-backed Express app. Public read-only API for the
  manifest and update feed; session-authenticated admin panel for
  publishing addons, WTF files, news, and launcher releases.
