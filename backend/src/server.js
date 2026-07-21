require('dotenv').config()
const path = require('path')
const express = require('express')
const session = require('express-session')
const methodOverride = require('method-override')

const apiRoutes = require('./routes/api')
const { router: adminRoutes } = require('./routes/admin')
const { UPLOAD_ROOT } = require('./files')

const app = express()

app.set('view engine', 'ejs')
app.set('views', path.join(__dirname, 'views'))

app.use(express.urlencoded({ extended: true }))
app.use(express.json())
app.use(methodOverride('_method'))
app.use(
  session({
    secret: process.env.SESSION_SECRET || 'dev-secret-change-me',
    resave: false,
    saveUninitialized: false,
    cookie: { httpOnly: true, maxAge: 1000 * 60 * 60 * 12 },
  }),
)

app.use('/admin/public', express.static(path.join(__dirname, 'public')))
app.use('/files', express.static(UPLOAD_ROOT, { maxAge: '1h' }))

app.use('/api', apiRoutes)
app.use('/admin', adminRoutes)

app.get('/', (_req, res) => res.redirect('/admin'))

const port = process.env.PORT || 8787
app.listen(port, () => {
  console.log(`SoT Guild Launcher backend listening on http://localhost:${port}`)
  console.log(`Admin panel: http://localhost:${port}/admin`)
  console.log(`Manifest API: http://localhost:${port}/api/manifest`)
})
