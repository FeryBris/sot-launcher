const express = require('express')
const db = require('../db')
const { publicUrl } = require('../files')

const router = express.Router()

router.get('/manifest', (_req, res) => {
  const addonRows = db.prepare('SELECT * FROM addons ORDER BY category, name').all()
  const wtfRows = db.prepare('SELECT * FROM wtf_files ORDER BY name').all()
  const newsRows = db.prepare('SELECT * FROM news ORDER BY date DESC, id DESC LIMIT 20').all()

  res.json({
    addons: addonRows.map((a) => ({
      name: a.name,
      version: a.version,
      url: publicUrl('addons', a.filename),
      sha256: a.sha256,
      category: a.category,
      changelog: a.changelog || '',
    })),
    wtfFiles: wtfRows.map((w) => ({
      name: w.name,
      version: w.version,
      url: publicUrl('wtf', w.filename),
      sha256: w.sha256,
      targetPath: w.target_path,
    })),
    news: newsRows.map((n) => ({
      title: n.title,
      body: n.body,
      date: n.date,
    })),
  })
})

router.get('/launcher/latest.json', (_req, res) => {
  const release = db.prepare('SELECT * FROM releases ORDER BY id DESC LIMIT 1').get()
  if (!release) {
    return res.status(404).json({ error: 'no releases published yet' })
  }
  const platformRows = db.prepare('SELECT * FROM release_platforms WHERE release_id = ?').all(release.id)
  if (platformRows.length === 0) {
    return res.status(404).json({ error: 'latest release has no platform artifacts' })
  }

  const platforms = {}
  for (const p of platformRows) {
    platforms[p.platform] = {
      signature: p.signature,
      url: publicUrl('launcher', p.filename),
    }
  }

  res.json({
    version: release.version,
    notes: release.notes || '',
    pub_date: release.pub_date,
    platforms,
  })
})

module.exports = router
