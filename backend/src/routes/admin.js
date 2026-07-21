const express = require('express')
const db = require('../db')
const { verifyLogin, requireAuth } = require('../auth')
const { uploaderFor, sha256File, removeFile } = require('../files')

const router = express.Router()

const addonUpload = uploaderFor('addons')
const wtfUpload = uploaderFor('wtf')
const releaseUpload = uploaderFor('launcher')

const PLATFORMS = ['windows-x86_64', 'linux-x86_64', 'darwin-x86_64', 'darwin-aarch64']

// ---- auth ----

router.get('/login', (req, res) => {
  if (req.session.userId) return res.redirect('/admin')
  res.render('login', { error: null })
})

router.post('/login', (req, res) => {
  const { username, password } = req.body
  const user = verifyLogin(username || '', password || '')
  if (!user) return res.render('login', { error: 'Invalid username or password' })
  req.session.userId = user.id
  req.session.username = user.username
  res.redirect('/admin')
})

router.post('/logout', (req, res) => {
  req.session.destroy(() => res.redirect('/admin/login'))
})

router.use(requireAuth)

// ---- dashboard ----

router.get('/', (_req, res) => {
  const counts = {
    addons: db.prepare('SELECT COUNT(*) c FROM addons').get().c,
    wtf: db.prepare('SELECT COUNT(*) c FROM wtf_files').get().c,
    news: db.prepare('SELECT COUNT(*) c FROM news').get().c,
    releases: db.prepare('SELECT COUNT(*) c FROM releases').get().c,
  }
  res.render('dashboard', { counts })
})

// ---- addons ----

router.get('/addons', (_req, res) => {
  const addons = db.prepare('SELECT * FROM addons ORDER BY category, name').all()
  res.render('addons/list', { addons })
})

router.get('/addons/new', (_req, res) => {
  res.render('addons/form', { addon: null })
})

router.post('/addons', addonUpload.single('file'), (req, res) => {
  const { name, version, category, changelog } = req.body
  if (!req.file) return res.status(400).send('An addon .zip file is required')
  const sha256 = sha256File(req.file.path)
  db.prepare(
    `INSERT INTO addons (name, version, filename, sha256, category, changelog, updated_at)
     VALUES (?, ?, ?, ?, ?, ?, datetime('now'))`,
  ).run(name, version, req.file.filename, sha256, category, changelog || '')
  res.redirect('/admin/addons')
})

router.get('/addons/:id/edit', (req, res) => {
  const addon = db.prepare('SELECT * FROM addons WHERE id = ?').get(req.params.id)
  if (!addon) return res.status(404).send('Not found')
  res.render('addons/form', { addon })
})

router.put('/addons/:id', addonUpload.single('file'), (req, res) => {
  const existing = db.prepare('SELECT * FROM addons WHERE id = ?').get(req.params.id)
  if (!existing) return res.status(404).send('Not found')
  const { name, version, category, changelog } = req.body

  let filename = existing.filename
  let sha256 = existing.sha256
  if (req.file) {
    removeFile('addons', existing.filename)
    filename = req.file.filename
    sha256 = sha256File(req.file.path)
  }

  db.prepare(
    `UPDATE addons SET name = ?, version = ?, filename = ?, sha256 = ?, category = ?, changelog = ?, updated_at = datetime('now')
     WHERE id = ?`,
  ).run(name, version, filename, sha256, category, changelog || '', req.params.id)
  res.redirect('/admin/addons')
})

router.delete('/addons/:id', (req, res) => {
  const existing = db.prepare('SELECT * FROM addons WHERE id = ?').get(req.params.id)
  if (existing) {
    removeFile('addons', existing.filename)
    db.prepare('DELETE FROM addons WHERE id = ?').run(req.params.id)
  }
  res.redirect('/admin/addons')
})

// ---- wtf files ----

router.get('/wtf', (_req, res) => {
  const files = db.prepare('SELECT * FROM wtf_files ORDER BY name').all()
  res.render('wtf/list', { files })
})

router.get('/wtf/new', (_req, res) => {
  res.render('wtf/form', { item: null })
})

router.post('/wtf', wtfUpload.single('file'), (req, res) => {
  const { name, version, target_path } = req.body
  if (!req.file) return res.status(400).send('A file is required')
  const sha256 = sha256File(req.file.path)
  db.prepare(
    `INSERT INTO wtf_files (name, version, filename, sha256, target_path, updated_at)
     VALUES (?, ?, ?, ?, ?, datetime('now'))`,
  ).run(name, version, req.file.filename, sha256, target_path)
  res.redirect('/admin/wtf')
})

router.get('/wtf/:id/edit', (req, res) => {
  const item = db.prepare('SELECT * FROM wtf_files WHERE id = ?').get(req.params.id)
  if (!item) return res.status(404).send('Not found')
  res.render('wtf/form', { item })
})

router.put('/wtf/:id', wtfUpload.single('file'), (req, res) => {
  const existing = db.prepare('SELECT * FROM wtf_files WHERE id = ?').get(req.params.id)
  if (!existing) return res.status(404).send('Not found')
  const { name, version, target_path } = req.body

  let filename = existing.filename
  let sha256 = existing.sha256
  if (req.file) {
    removeFile('wtf', existing.filename)
    filename = req.file.filename
    sha256 = sha256File(req.file.path)
  }

  db.prepare(
    `UPDATE wtf_files SET name = ?, version = ?, filename = ?, sha256 = ?, target_path = ?, updated_at = datetime('now')
     WHERE id = ?`,
  ).run(name, version, filename, sha256, target_path, req.params.id)
  res.redirect('/admin/wtf')
})

router.delete('/wtf/:id', (req, res) => {
  const existing = db.prepare('SELECT * FROM wtf_files WHERE id = ?').get(req.params.id)
  if (existing) {
    removeFile('wtf', existing.filename)
    db.prepare('DELETE FROM wtf_files WHERE id = ?').run(req.params.id)
  }
  res.redirect('/admin/wtf')
})

// ---- news ----

router.get('/news', (_req, res) => {
  const items = db.prepare('SELECT * FROM news ORDER BY date DESC, id DESC').all()
  res.render('news/list', { items })
})

router.get('/news/new', (_req, res) => {
  res.render('news/form', { item: null })
})

router.post('/news', (req, res) => {
  const { title, body, date } = req.body
  db.prepare('INSERT INTO news (title, body, date) VALUES (?, ?, ?)').run(title, body, date)
  res.redirect('/admin/news')
})

router.get('/news/:id/edit', (req, res) => {
  const item = db.prepare('SELECT * FROM news WHERE id = ?').get(req.params.id)
  if (!item) return res.status(404).send('Not found')
  res.render('news/form', { item })
})

router.put('/news/:id', (req, res) => {
  const { title, body, date } = req.body
  db.prepare('UPDATE news SET title = ?, body = ?, date = ? WHERE id = ?').run(title, body, date, req.params.id)
  res.redirect('/admin/news')
})

router.delete('/news/:id', (req, res) => {
  db.prepare('DELETE FROM news WHERE id = ?').run(req.params.id)
  res.redirect('/admin/news')
})

// ---- launcher releases ----

router.get('/releases', (_req, res) => {
  const releases = db.prepare('SELECT * FROM releases ORDER BY id DESC').all()
  const platformsByRelease = {}
  for (const r of releases) {
    platformsByRelease[r.id] = db.prepare('SELECT * FROM release_platforms WHERE release_id = ?').all(r.id)
  }
  res.render('releases/list', { releases, platformsByRelease })
})

router.get('/releases/new', (_req, res) => {
  res.render('releases/form')
})

router.post('/releases', (req, res) => {
  const { version, notes, pub_date } = req.body
  db.prepare('INSERT INTO releases (version, notes, pub_date) VALUES (?, ?, ?)').run(
    version,
    notes || '',
    pub_date || new Date().toISOString(),
  )
  res.redirect('/admin/releases')
})

router.get('/releases/:id', (req, res) => {
  const release = db.prepare('SELECT * FROM releases WHERE id = ?').get(req.params.id)
  if (!release) return res.status(404).send('Not found')
  const platforms = db.prepare('SELECT * FROM release_platforms WHERE release_id = ?').all(req.params.id)
  const usedPlatforms = new Set(platforms.map((p) => p.platform))
  const availablePlatforms = PLATFORMS.filter((p) => !usedPlatforms.has(p))
  res.render('releases/detail', { release, platforms, availablePlatforms })
})

router.post(
  '/releases/:id/platforms',
  releaseUpload.fields([{ name: 'installer', maxCount: 1 }, { name: 'signature_file', maxCount: 1 }]),
  (req, res) => {
    const release = db.prepare('SELECT * FROM releases WHERE id = ?').get(req.params.id)
    if (!release) return res.status(404).send('Not found')

    const { platform, signature_text } = req.body
    const installer = req.files?.installer?.[0]
    if (!installer) return res.status(400).send('Installer file is required')

    let signature = (signature_text || '').trim()
    const sigFile = req.files?.signature_file?.[0]
    if (sigFile) {
      signature = require('fs').readFileSync(sigFile.path, 'utf8').trim()
      removeFile('launcher', sigFile.filename)
    }
    if (!signature) return res.status(400).send('Signature (paste text or upload .sig file) is required')

    db.prepare(
      `INSERT INTO release_platforms (release_id, platform, filename, signature)
       VALUES (?, ?, ?, ?)
       ON CONFLICT(release_id, platform) DO UPDATE SET filename = excluded.filename, signature = excluded.signature`,
    ).run(release.id, platform, installer.filename, signature)

    res.redirect(`/admin/releases/${release.id}`)
  },
)

router.delete('/releases/:id', (req, res) => {
  const platforms = db.prepare('SELECT * FROM release_platforms WHERE release_id = ?').all(req.params.id)
  for (const p of platforms) removeFile('launcher', p.filename)
  db.prepare('DELETE FROM releases WHERE id = ?').run(req.params.id)
  res.redirect('/admin/releases')
})

router.delete('/releases/:id/platforms/:platformId', (req, res) => {
  const p = db.prepare('SELECT * FROM release_platforms WHERE id = ? AND release_id = ?').get(
    req.params.platformId,
    req.params.id,
  )
  if (p) {
    removeFile('launcher', p.filename)
    db.prepare('DELETE FROM release_platforms WHERE id = ?').run(p.id)
  }
  res.redirect(`/admin/releases/${req.params.id}`)
})

module.exports = { router, PLATFORMS }
