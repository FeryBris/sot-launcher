const fs = require('fs')
const path = require('path')
const crypto = require('crypto')
const multer = require('multer')

const UPLOAD_ROOT = path.join(__dirname, '..', 'uploads')

function uploaderFor(subdir) {
  const dest = path.join(UPLOAD_ROOT, subdir)
  fs.mkdirSync(dest, { recursive: true })
  const storage = multer.diskStorage({
    destination: dest,
    filename(_req, file, cb) {
      const safeBase = path.basename(file.originalname).replace(/[^a-zA-Z0-9._-]/g, '_')
      cb(null, `${Date.now()}-${crypto.randomBytes(4).toString('hex')}-${safeBase}`)
    },
  })
  return multer({ storage, limits: { fileSize: 500 * 1024 * 1024 } })
}

function sha256File(filePath) {
  const hash = crypto.createHash('sha256')
  hash.update(fs.readFileSync(filePath))
  return hash.digest('hex')
}

function publicUrl(subdir, filename) {
  const base = (process.env.PUBLIC_BASE_URL || '').replace(/\/$/, '')
  return `${base}/files/${subdir}/${filename}`
}

function removeFile(subdir, filename) {
  if (!filename) return
  const p = path.join(UPLOAD_ROOT, subdir, filename)
  fs.rm(p, { force: true }, () => {})
}

module.exports = { UPLOAD_ROOT, uploaderFor, sha256File, publicUrl, removeFile }
