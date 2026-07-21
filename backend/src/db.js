const path = require('path')
const crypto = require('crypto')
const Database = require('better-sqlite3')
const bcrypt = require('bcryptjs')

const db = new Database(path.join(__dirname, '..', 'data', 'launcher.db'))
db.pragma('journal_mode = WAL')
db.pragma('foreign_keys = ON')

db.exec(`
  CREATE TABLE IF NOT EXISTS admin_users (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    username TEXT UNIQUE NOT NULL,
    password_hash TEXT NOT NULL
  );

  CREATE TABLE IF NOT EXISTS addons (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT UNIQUE NOT NULL,
    version TEXT NOT NULL,
    filename TEXT NOT NULL,
    sha256 TEXT NOT NULL,
    category TEXT NOT NULL CHECK (category IN ('mandatory', 'recommended', 'optional')),
    changelog TEXT DEFAULT '',
    updated_at TEXT NOT NULL
  );

  CREATE TABLE IF NOT EXISTS wtf_files (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT UNIQUE NOT NULL,
    version TEXT NOT NULL,
    filename TEXT NOT NULL,
    sha256 TEXT NOT NULL,
    target_path TEXT NOT NULL,
    updated_at TEXT NOT NULL
  );

  CREATE TABLE IF NOT EXISTS news (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    date TEXT NOT NULL
  );

  CREATE TABLE IF NOT EXISTS releases (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    version TEXT UNIQUE NOT NULL,
    notes TEXT DEFAULT '',
    pub_date TEXT NOT NULL
  );

  CREATE TABLE IF NOT EXISTS release_platforms (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    release_id INTEGER NOT NULL REFERENCES releases(id) ON DELETE CASCADE,
    platform TEXT NOT NULL,
    filename TEXT NOT NULL,
    signature TEXT NOT NULL,
    UNIQUE(release_id, platform)
  );
`)

function seedAdmin() {
  const existing = db.prepare('SELECT id FROM admin_users LIMIT 1').get()
  if (existing) return

  const username = process.env.ADMIN_USERNAME || 'admin'
  const password = process.env.ADMIN_PASSWORD || crypto.randomBytes(9).toString('base64url')
  const hash = bcrypt.hashSync(password, 12)

  db.prepare('INSERT INTO admin_users (username, password_hash) VALUES (?, ?)').run(username, hash)

  if (!process.env.ADMIN_PASSWORD) {
    console.log('')
    console.log('=========================================================')
    console.log(' Created initial admin account')
    console.log(` username: ${username}`)
    console.log(` password: ${password}`)
    console.log(' (set ADMIN_PASSWORD in .env to control this next time)')
    console.log('=========================================================')
    console.log('')
  }
}

seedAdmin()

module.exports = db
