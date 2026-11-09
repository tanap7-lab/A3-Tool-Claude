# Resets the dev database, creates the first admin, clears local app state, and runs the end-to-end test.
# This erases the dev database, so it refuses to run against anything but the local .dev cluster.
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

Get-Process a3-sheet -ErrorAction SilentlyContinue | Stop-Process -Force
& .\scripts\dev-db.ps1 reset | Out-Null
$vals = @{}
Get-Content '.dev\dev.env' | ForEach-Object { $k, $v = $_ -split '=', 2; $vals[$k] = $v }
[IO.File]::WriteAllText("$root\.env", "DATABASE_URL=$($vals.DATABASE_URL)`n")
$env:DATABASE_URL = $vals.DATABASE_URL
sqlx migrate run --source crates\a3-core\migrations | Out-Null

$env:A3_E2E_PASSWORD = 'e2e-admin-password'
$env:A3_ADMIN_PASSWORD = $env:A3_E2E_PASSWORD
cargo run -q -p a3-core --bin a3-admin -- bootstrap paul 'Paul' 'paul@example.com'

foreach ($d in @("$env:APPDATA\com.a3sheet.desktop", "$env:LOCALAPPDATA\com.a3sheet.desktop\recovery")) {
  Remove-Item -Recurse -Force $d -ErrorAction SilentlyContinue
}
Remove-Item -Recurse -Force '.dev\e2e\shots', '.dev\e2e\exports' -ErrorAction SilentlyContinue
node e2e\run.mjs
exit $LASTEXITCODE
