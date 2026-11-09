# Starts the local development PostgreSQL from portable binaries in .dev\, creating it on first run.
# No administrator rights and no Windows service. pgAdmin can connect to localhost:5433.
#   .\scripts\dev-db.ps1          start, creating the cluster if needed
#   .\scripts\dev-db.ps1 stop     stop the server
#   .\scripts\dev-db.ps1 reset    delete the dev data and create it again
param([string]$Action = 'start')
$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $PSScriptRoot
$dev  = Join-Path $root '.dev'
$bin  = Join-Path $dev 'pg-extract\pgsql\bin'
$data = Join-Path $dev 'pgdata'
$envFile = Join-Path $dev 'dev.env'
$port = 5433

if (-not (Test-Path (Join-Path $bin 'postgres.exe'))) {
  New-Item -ItemType Directory -Force (Join-Path $dev 'dl\pg') | Out-Null
  $zip = Join-Path $dev 'dl\pg\pg.zip'
  if (-not (Test-Path $zip)) {
    Write-Host 'Downloading portable PostgreSQL 17...'
    curl.exe -sSL -o $zip 'https://get.enterprisedb.com/postgresql/postgresql-17.7-1-windows-x64-binaries.zip'
  }
  Expand-Archive -Path $zip -DestinationPath (Join-Path $dev 'pg-extract') -Force
}

function New-Secret { -join ((48..57) + (97..122) | Get-Random -Count 24 | ForEach-Object { [char]$_ }) }
function Test-Running { & "$bin\pg_ctl.exe" -D $data status *> $null; return $LASTEXITCODE -eq 0 }

if ($Action -eq 'stop' -or $Action -eq 'reset') {
  if ((Test-Path $data) -and (Test-Running)) { & "$bin\pg_ctl.exe" -D $data stop -m fast | Out-Null }
  if ($Action -eq 'stop') { Write-Host 'Stopped.'; return }
  Remove-Item -Recurse -Force $data -ErrorAction SilentlyContinue
  Remove-Item -Force $envFile -ErrorAction SilentlyContinue
}

$fresh = -not (Test-Path (Join-Path $data 'PG_VERSION'))
if ($fresh) {
  $super = New-Secret; $owner = New-Secret
  $pw = Join-Path $dev 'pw.tmp'
  [IO.File]::WriteAllText($pw, $super)
  & "$bin\initdb.exe" -D $data -U postgres --pwfile=$pw --auth=scram-sha-256 -E UTF8 --locale=C | Out-Null
  Remove-Item -Force $pw
  Add-Content (Join-Path $data 'postgresql.conf') "port = $port`nlisten_addresses = 'localhost'`nlogging_collector = off`n"
  $lines = @(
    "PGSUPERPASS=$super",
    "PGOWNERPASS=$owner",
    "DATABASE_URL=postgres://a3_owner:$owner@localhost:$port/a3sheet"
  )
  [IO.File]::WriteAllLines($envFile, $lines)
}

if (-not (Test-Running)) {
  # Detached: pg_ctl's server child would otherwise hold this shell's output pipe open forever.
  $p = Start-Process -FilePath "$bin\pg_ctl.exe" -ArgumentList @('-D', "`"$data`"", '-l', "`"$(Join-Path $dev 'pg.log')`"", '-w', 'start') -WindowStyle Hidden -PassThru
  $p.WaitForExit()
}

$vals = @{}
Get-Content $envFile | ForEach-Object { $k, $v = $_ -split '=', 2; $vals[$k] = $v }

if ($fresh) {
  $env:PGPASSWORD = $vals.PGSUPERPASS
  & "$bin\psql.exe" -h localhost -p $port -U postgres -d postgres -v ON_ERROR_STOP=1 -q `
    -c "CREATE ROLE a3_owner LOGIN CREATEROLE PASSWORD '$($vals.PGOWNERPASS)'" `
    -c 'CREATE DATABASE a3sheet OWNER a3_owner'
  Remove-Item Env:PGPASSWORD
}
Write-Host "PostgreSQL is running on localhost:$port. Details are in .dev\dev.env."
