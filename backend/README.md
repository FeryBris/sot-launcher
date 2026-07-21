# SoT Guild Launcher — Backend

Express + SQLite server that hosts the addon/WTF manifest, guild news, and
launcher self-update feed, plus an admin panel to manage all of it.

## Quick start

```bash
npm install
cp .env.example .env   # edit SESSION_SECRET, PUBLIC_BASE_URL, etc.
npm start
```

On first boot, if `ADMIN_PASSWORD` isn't set in `.env`, an admin account is
created and the generated password is printed to the console **once** — copy
it immediately. Log in at `http://localhost:8787/admin`.

## What it serves

- `GET /api/manifest` — addons, WTF/SavedVariables files, and news; consumed
  by the desktop launcher.
- `GET /api/launcher/latest.json` — Tauri updater feed. Returns the most
  recently created release in the admin panel, in the format
  `{ version, notes, pub_date, platforms: { "windows-x86_64": { signature, url } } }`.
- `/files/*` — static hosting for uploaded addon zips, WTF files, and
  launcher installers. `sha256` is computed server-side on upload, so the
  hash the client verifies against always matches the real file.

## Publishing a launcher update

1. Bump `version` in `src-tauri/tauri.conf.json` and run
   `pnpm tauri build` — with `TAURI_SIGNING_PRIVATE_KEY_PATH` (and
   `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` if set) pointed at
   `.tauri-keys/launcher.key`, this produces both the installer and a
   matching `.sig` file.
2. In the admin panel, create a new release with that version.
3. Open the release, pick a platform (e.g. `windows-x86_64`), and upload the
   installer plus its `.sig` file (or paste the signature text directly).
4. Running launchers will pick this up next time they check for updates.

## Notes

- SQLite database lives in `data/launcher.db` (gitignored). Back it up.
- Uploaded files live in `uploads/` (gitignored). Back it up too, or point
  it at network/object storage in production.
- This is a single-admin setup (one `admin_users` row seeded on first boot).
  There's no self-serve signup — add more rows directly in SQLite if a
  second officer needs access.
