# First-time setup of a development machine. Needs Rust, Node and the Visual Studio C++ build tools.
# No administrator rights are needed. Safe to run again.
#   .\scripts\setup-dev.ps1 -AdminLogin paul -AdminName "Paul" -AdminEmail paul@example.com
param(
  [string]$AdminLogin,
  [string]$AdminName,
  [string]$AdminEmail = ''
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

& .\scripts\fetch-pdfium.ps1
npm install --no-fund --no-audit
if (-not (Get-Command sqlx -ErrorAction SilentlyContinue)) {
  cargo install sqlx-cli --version 0.9.0 --no-default-features --features rustls,postgres --locked
}
& .\scripts\dev-db.ps1
$vals = @{}
Get-Content '.dev\dev.env' | ForEach-Object { $k, $v = $_ -split '=', 2; $vals[$k] = $v }
[IO.File]::WriteAllText("$root\.env", "DATABASE_URL=$($vals.DATABASE_URL)`n")
$env:DATABASE_URL = $vals.DATABASE_URL
sqlx migrate run --source crates\a3-core\migrations

if ($AdminLogin) {
  $secure = Read-Host "Password for $AdminLogin (at least 8 characters)" -AsSecureString
  $env:A3_ADMIN_PASSWORD = [Runtime.InteropServices.Marshal]::PtrToStringAuto([Runtime.InteropServices.Marshal]::SecureStringToBSTR($secure))
  $display = if ($AdminName) { $AdminName } else { $AdminLogin }
  cargo run -q -p a3-core --bin a3-admin -- bootstrap $AdminLogin $display $AdminEmail
  Remove-Item Env:A3_ADMIN_PASSWORD
}
Write-Host 'Ready. Start the app with: npx tauri dev'
