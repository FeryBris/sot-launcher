@echo off
:: One-time setup so Windows trusts SoT Launcher installers without a
:: SmartScreen/"Unknown Publisher" warning. Run this once, ever, per PC -
:: not needed again for future launcher updates. Requires admin (Trusted
:: Root Certification Authorities is a machine-wide store), so this
:: re-launches itself elevated if needed.

net session >nul 2>&1
if %errorLevel% neq 0 (
    echo Requesting administrator privileges...
    powershell -NoProfile -Command "Start-Process -FilePath '%~f0' -Verb RunAs"
    exit /b
)

powershell -NoProfile -ExecutionPolicy Bypass -Command "Import-Certificate -FilePath '%~dp0SoTLauncher.cer' -CertStoreLocation Cert:\LocalMachine\Root" >nul
if %errorLevel% equ 0 (
    echo.
    echo Done - SoT Launcher's certificate is now trusted on this PC.
    echo You can now run the installer.
) else (
    echo.
    echo Something went wrong importing the certificate.
    echo Make sure SoTLauncher.cer is in the same folder as this script.
)

pause
