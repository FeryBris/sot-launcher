# Windows code-signing certificate

Self-signed (not CA-issued) — signs Windows installers so Windows trusts
them on machines that have imported `SoTLauncher.cer`, without paying for
a real certificate. See the "why" discussion in project history if you
want the full reasoning; short version: this is a private-app-for-friends
tradeoff, not something you'd want for a public release.

## Files here (committed, public)

- `SoTLauncher.cer` — the certificate's public half. Distribute this
  alongside every release.
- `install-cert.bat` — run once, ever, per PC. Self-elevates, imports
  `SoTLauncher.cer` into Windows' Trusted Root store. After that, every
  future signed installer is trusted automatically — this never needs to
  be run again on that PC.

## What's NOT here

The private half (`.windows-cert/SoTLauncher.pfx`, gitignored) never gets
committed. It only exists:
- Locally, wherever it was generated.
- As the `WINDOWS_CERT_PFX_BASE64` GitHub Actions secret (base64-encoded),
  used by `build-release.yml` to sign Windows builds in CI.

## One-time GitHub setup

In the repo's Settings > Secrets and variables > Actions, add:

- `WINDOWS_CERT_PFX_BASE64` — the `.pfx` file, base64-encoded as one line.
- `WINDOWS_CERT_PASSWORD` — the password it was exported with.

## Regenerating / rotating

Only do this if the private key is genuinely lost — like the updater
signing key, rotating means every PC that already ran `install-cert.bat`
needs to run it again for the *new* cert, or existing installs still
running an old, differently-signed build will still work (unlike the
updater key, there's no auto-update chain to break — this only affects
whether a *fresh* install/download avoids the SmartScreen prompt), but
newly downloaded installers won't be trusted anywhere until re-imported.

```powershell
$cert = New-SelfSignedCertificate `
  -Type CodeSigningCert `
  -Subject "CN=Stalkers of Time" `
  -FriendlyName "SoT Launcher Code Signing" `
  -CertStoreLocation "Cert:\CurrentUser\My" `
  -NotAfter (Get-Date).AddYears(15) `
  -KeyExportPolicy Exportable `
  -KeyAlgorithm RSA `
  -KeyLength 2048 `
  -KeyUsage DigitalSignature

$plainPassword = [System.Web.Security.Membership]::GeneratePassword(32, 8)
$securePassword = ConvertTo-SecureString -String $plainPassword -Force -AsPlainText

Export-PfxCertificate -Cert $cert -FilePath ".windows-cert\SoTLauncher.pfx" -Password $securePassword
Export-Certificate -Cert $cert -FilePath "packaging\windows-cert\SoTLauncher.cer" -Type CERT

Write-Output "PFX password (save this - goes in the WINDOWS_CERT_PASSWORD secret): $plainPassword"
```

Then update `certificateThumbprint` in `src-tauri/tauri.conf.json` to the
new cert's thumbprint (`$cert.Thumbprint` above), and update both GitHub
secrets.
