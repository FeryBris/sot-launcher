# SoT Launcher

Desktop launcher for the guild: syncs mandatory/recommended/optional addons
and WTF/SavedVariables files from the guild server, self-updates, and
launches WoW.exe. Built with [Tauri](https://tauri.app) (Rust + Vue 3 +
TypeScript).

The manifest API and admin panel live in the **wotlk** Laravel site
(`app/Http/Controllers/Api/Launcher`, `.../Admin/Launcher`,
`resources/scripts/pages/Admin/Launcher`) rather than in this repo — see that
project for the backend. The `backend/` folder here is an earlier standalone
Node/Express implementation, superseded by the Laravel integration; see
`backend/README.md` for its status.

## Layout

- `src/`, `src-tauri/` — the desktop client (this root project)
- `backend/` — superseded standalone manifest API + admin panel (see `backend/README.md`)

## Client: local development

Requires Rust, Node, and pnpm. On Linux, Tauri also needs system webkit2gtk/
GTK dev packages installed once (see the
[Tauri prerequisites docs](https://v2.tauri.app/start/prerequisites/)).

```bash
pnpm install
pnpm tauri dev
```

Point the launcher at your backend from Settings (server URL + API key), or
set the defaults in `src-tauri/src/types.rs` (`default_api_base_url`).

## Client: building a release

```bash
pnpm tauri:build:signed
```

Produces a signed installer (`.msi`/`.exe` on Windows) plus a `.sig` file,
using the keypair in `.tauri-keys/` (see below). Upload both through the
Laravel admin panel's Launcher Releases section to ship a self-update.

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
  disk, extracts addons into `Interface/AddOns` (tracking exactly which
  files each addon wrote, so disabling one removes precisely those files),
  places WTF files at their exact target path, and launches `WoW.exe` (or a
  custom launch command for Lutris/Bottles/Wine setups). Progress streams to
  the UI via Tauri events.
- **Client** (Vue): Home (guild news), Addons (mandatory/recommended/
  optional, each individually toggleable with live sync status), Settings
  (game folder, launch command, close-on-launch/auto-launch, server URL, API
  key), and a persistent Play/Sync bar with self-update banner.
- **Backend** (in the wotlk repo): public API gated by a shared
  `X-API-KEY` (`GET /api/launcher/manifest`, `GET /api/launcher/latest.json`
  in the Tauri updater feed format); admin panel under the existing
  `role:admin` auth for publishing addons, WTF files, and launcher releases
  (with per-platform installers + signatures).
