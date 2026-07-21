const bcrypt = require('bcryptjs')
const db = require('./db')

function verifyLogin(username, password) {
  const user = db.prepare('SELECT * FROM admin_users WHERE username = ?').get(username)
  if (!user) return null
  if (!bcrypt.compareSync(password, user.password_hash)) return null
  return user
}

function requireAuth(req, res, next) {
  if (req.session && req.session.userId) return next()
  return res.redirect('/admin/login')
}

module.exports = { verifyLogin, requireAuth }
