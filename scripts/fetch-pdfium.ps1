# Downloads the PDFium build that matches the pdfium-render crate's `pdfium_latest` feature (0.9.5 binds
# build 8125), and puts pdfium.dll where the Tauri bundle picks it up. Update both together.
$ErrorActionPreference = 'Stop'
$build = '8125'
$root = Split-Path -Parent $PSScriptRoot
$dest = Join-Path $root 'src-tauri\resources'
$dll = Join-Path $dest 'pdfium.dll'
$stamp = Join-Path $dest 'pdfium.build'

if ((Test-Path $dll) -and (Test-Path $stamp) -and ((Get-Content $stamp -Raw).Trim() -eq $build)) {
  Write-Host "pdfium.dll build $build is already in place."
  return
}
New-Item -ItemType Directory -Force $dest | Out-Null
$work = Join-Path ([IO.Path]::GetTempPath()) ("pdfium-" + [guid]::NewGuid())
New-Item -ItemType Directory -Force $work | Out-Null
$tgz = Join-Path $work 'pdfium.tgz'
curl.exe -sSL -f -o $tgz "https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F$build/pdfium-win-x64.tgz"
if ($LASTEXITCODE -ne 0) { throw "Could not download PDFium build $build." }
tar.exe -xzf $tgz -C $work
Copy-Item (Join-Path $work 'bin\pdfium.dll') $dll -Force
Set-Content -Path $stamp -Value $build -NoNewline
Remove-Item -Recurse -Force $work
Write-Host "pdfium.dll build $build is in src-tauri\resources."
